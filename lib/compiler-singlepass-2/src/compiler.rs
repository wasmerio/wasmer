//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::codegen::{CodegenError, FuncGen};
use crate::config::Singlepass;
use rayon::prelude::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use wasmer_compiler::TrapInformation;
use wasmer_compiler::{
    Architecture, CompileModuleInfo, CompilerConfig, MiddlewareBinaryReader, ModuleMiddlewareChain,
    ModuleTranslationState, OperatingSystem, Target,
};
use wasmer_compiler::{
    Compilation, CompileError, CompiledFunction, Compiler, SectionIndex,
    FunctionBody, FunctionBodyData, CustomSection, CustomSectionProtection, SectionBody};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, TableIndex};
use wasmer_vm::{ModuleInfo, TrapCode, VMOffsets};

use crate::machine::Machine;
use crate::machine_aarch64::Aarch64Machine;
use crate::machine_x64::X64Machine;

/// A compiler that compiles a WebAssembly module with Singlepass.
/// It does the compilation in one pass
pub struct SinglepassCompiler {
    config: Singlepass,
}

impl SinglepassCompiler {
    /// Creates a new Singlepass compiler
    pub fn new(config: Singlepass) -> Self {
        Self { config }
    }

    /// Gets the config for this Compiler
    fn config(&self) -> &Singlepass {
        &self.config
    }
    
    fn gen_function<M: Machine>(&self, input: &FunctionBodyData,
        vmoffsets: &VMOffsets, compile_info: &CompileModuleInfo, i: LocalFunctionIndex)
        -> Result<CompiledFunction, CompileError> {
        
        let middleware_chain = self
            .config
            .middlewares
            .generate_function_middleware_chain(i);
        let mut reader = MiddlewareBinaryReader::new_with_offset(input.data, input.module_offset);
        reader.set_middleware_chain(middleware_chain);

        // This local list excludes arguments.
        let mut locals = vec![];
        let num_locals = reader.read_local_count()?;
        for _ in 0..num_locals {
            let (count, ty) = reader.read_local_decl()?;
            for _ in 0..count {
                locals.push(ty);
            }
        }
        
        let machine = M::new();
        let mut generator = FuncGen::new(
            machine, &compile_info.module, &self.config, &vmoffsets,
            &compile_info.memory_styles, &compile_info.table_styles, i, &locals,
        ).map_err(to_compile_error)?;
        
        while generator.has_control_frames() {
            generator.set_srcloc(reader.original_position() as u32);
            let op = reader.read_operator()?;
            generator.feed_operator(op).map_err(to_compile_error)?;
        }
    
        Ok(generator.finalize(&input))
    }
}

struct Generator {
    function: fn(&SinglepassCompiler, &FunctionBodyData, &VMOffsets, &CompileModuleInfo, LocalFunctionIndex)
        -> Result<CompiledFunction, CompileError>,
    std_trampoline: fn(sig: &FunctionType) -> Vec<u8>,
    std_dynamic_import_trampoline: fn(vmoffsets: &VMOffsets, sig: &FunctionType) -> Vec<u8>,
    import_call_trampoline: fn(vmoffsets: &VMOffsets, index: FunctionIndex, sig: &FunctionType) -> Vec<u8>
}

impl Generator {
    fn gen_function(&self, compiler: &SinglepassCompiler, input: &FunctionBodyData,
        vmoffsets: &VMOffsets, compile_info: &CompileModuleInfo, i: LocalFunctionIndex)
        -> Result<CompiledFunction, CompileError> {
        (self.function)(compiler, input, vmoffsets, compile_info, i)
    }
    fn gen_std_trampoline(&self, sig: &FunctionType) -> FunctionBody {
        FunctionBody {
            body: (self.std_trampoline)(sig),
            unwind_info: None,
        }
    }
    fn gen_std_dynamic_import_trampoline(&self, vmoffsets: &VMOffsets, sig: &FunctionType) -> FunctionBody {
        FunctionBody {
            body: (self.std_dynamic_import_trampoline)(vmoffsets, sig),
            unwind_info: None,
        }
    }
    fn gen_import_call_trampoline(&self, vmoffsets: &VMOffsets, index: FunctionIndex, sig: &FunctionType) -> CustomSection {
        CustomSection {
            protection: CustomSectionProtection::ReadExecute,
            bytes: SectionBody::new_with_vec((self.import_call_trampoline)(vmoffsets, index, sig)),
            relocations: vec![],
        }
    }
}

impl Compiler for SinglepassCompiler {
    /// Compile the module using Singlepass, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        target: &Target,
        compile_info: &mut CompileModuleInfo,
        _module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError> {
        if target.triple().operating_system == OperatingSystem::Windows {
            return Err(CompileError::UnsupportedTarget(
                OperatingSystem::Windows.to_string(),
            ));
        }
        if let Architecture::X86_32(arch) = target.triple().architecture {
            return Err(CompileError::UnsupportedTarget(arch.to_string()));
        }
        if compile_info.features.multi_value {
            return Err(CompileError::UnsupportedFeature("multivalue".to_string()));
        }

        let gen = match target.triple().architecture {
            Architecture::Aarch64(_) => {
                Generator {
                    function: Self::gen_function::<Aarch64Machine>,
                    std_trampoline: Aarch64Machine::gen_std_trampoline,
                    import_call_trampoline: Aarch64Machine::gen_import_call_trampoline,
                    std_dynamic_import_trampoline: Aarch64Machine::gen_std_dynamic_import_trampoline,
                }
            },
            Architecture::X86_64 => {
                Generator {
                    function: Self::gen_function::<X64Machine>,
                    std_trampoline: X64Machine::gen_std_trampoline,
                    import_call_trampoline: X64Machine::gen_import_call_trampoline,
                    std_dynamic_import_trampoline: X64Machine::gen_std_dynamic_import_trampoline,
                }
            },
            arch => {
                return Err(CompileError::UnsupportedTarget(arch.to_string()));
            },
        };

        let mut module = (*compile_info.module).clone();
        self.config.middlewares.apply_on_module_info(&mut module);
        compile_info.module = Arc::new(module);
        let vmoffsets = VMOffsets::new(8, &compile_info.module);
        let module = &compile_info.module;
        let import_trampolines: PrimaryMap<SectionIndex, _> = (0..module.num_imported_functions)
            .map(FunctionIndex::new)
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|i| gen.gen_import_call_trampoline(&vmoffsets, i, &module.signatures[module.functions[i]]))
            .collect::<Vec<_>>()
            .into_iter()
            .collect();
        let functions = function_body_inputs
            .iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .iter()// .par_iter()
            .map(|(i, input)| gen.gen_function(&self, input, &vmoffsets, &compile_info, *i))
            .collect::<Result<Vec<CompiledFunction>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<LocalFunctionIndex, CompiledFunction>>();
        let function_call_trampolines = module
            .signatures
            .values()
            .collect::<Vec<_>>()
            .par_iter()
            .cloned()
            .map(|sig| gen.gen_std_trampoline(sig))
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<PrimaryMap<_, _>>();
        let dynamic_function_trampolines = module
            .imported_function_types()
            .collect::<Vec<_>>()
            .par_iter()
            .map(|func_type| gen.gen_std_dynamic_import_trampoline(&vmoffsets, func_type))
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<PrimaryMap<FunctionIndex, FunctionBody>>();

        Ok(Compilation::new(
            functions,
            import_trampolines,
            function_call_trampolines,
            dynamic_function_trampolines,
            None,
        ))
    }
}

trait ToCompileError {
    fn to_compile_error(self) -> CompileError;
}

impl ToCompileError for CodegenError {
    fn to_compile_error(self) -> CompileError {
        CompileError::Codegen(self.message)
    }
}

fn to_compile_error<T: ToCompileError>(x: T) -> CompileError {
    x.to_compile_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use target_lexicon::triple;
    use wasmer_compiler::{CpuFeature, Features, Triple};
    use wasmer_vm::{MemoryStyle, TableStyle};

    fn dummy_compilation_ingredients<'a>() -> (
        CompileModuleInfo,
        ModuleTranslationState,
        PrimaryMap<LocalFunctionIndex, FunctionBodyData<'a>>,
    ) {
        let compile_info = CompileModuleInfo {
            features: Features::new(),
            module: Arc::new(ModuleInfo::new()),
            memory_styles: PrimaryMap::<MemoryIndex, MemoryStyle>::new(),
            table_styles: PrimaryMap::<TableIndex, TableStyle>::new(),
        };
        let module_translation = ModuleTranslationState::new();
        let function_body_inputs = PrimaryMap::<LocalFunctionIndex, FunctionBodyData<'_>>::new();
        (compile_info, module_translation, function_body_inputs)
    }

    #[test]
    fn errors_for_unsupported_targets() {
        let compiler = SinglepassCompiler::new(Singlepass::default());

        // Compile for win64
        let win64 = Target::new(triple!("x86_64-pc-windows-msvc"), CpuFeature::for_host());
        let (mut info, translation, inputs) = dummy_compilation_ingredients();
        let result = compiler.compile_module(&win64, &mut info, &translation, inputs);
        match result.unwrap_err() {
            CompileError::UnsupportedTarget(name) => assert_eq!(name, "windows"),
            error => panic!("Unexpected error: {:?}", error),
        };

        // Compile for 32bit Linux
        let linux32 = Target::new(triple!("i686-unknown-linux-gnu"), CpuFeature::for_host());
        let (mut info, translation, inputs) = dummy_compilation_ingredients();
        let result = compiler.compile_module(&linux32, &mut info, &translation, inputs);
        match result.unwrap_err() {
            CompileError::UnsupportedTarget(name) => assert_eq!(name, "i686"),
            error => panic!("Unexpected error: {:?}", error),
        };

        // Compile for win32
        let win32 = Target::new(triple!("i686-pc-windows-gnu"), CpuFeature::for_host());
        let (mut info, translation, inputs) = dummy_compilation_ingredients();
        let result = compiler.compile_module(&win32, &mut info, &translation, inputs);
        match result.unwrap_err() {
            CompileError::UnsupportedTarget(name) => assert_eq!(name, "windows"), // Windows should be checked before architecture
            error => panic!("Unexpected error: {:?}", error),
        };
    }
}
