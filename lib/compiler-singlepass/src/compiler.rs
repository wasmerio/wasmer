//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::codegen::FuncGen;
use crate::config::Singlepass;
use crate::machine::Machine;
use crate::machine::{
    gen_import_call_trampoline, gen_std_dynamic_import_trampoline, gen_std_trampoline, CodegenError,
};
use crate::machine_arm64::MachineARM64;
use crate::machine_x64::MachineX86_64;
use loupe::MemoryUsage;
#[cfg(feature = "rayon")]
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::sync::Arc;
use wasmer_compiler::{
    Architecture, CallingConvention, Compilation, CompileError, CompileModuleInfo,
    CompiledFunction, Compiler, CompilerConfig, CpuFeature, FunctionBinaryReader, FunctionBody,
    FunctionBodyData, MiddlewareBinaryReader, ModuleMiddleware, ModuleMiddlewareChain,
    ModuleTranslationState, OperatingSystem, SectionIndex, Target, TrapInformation,
};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{
    FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, ModuleInfo, TableIndex,
};
use wasmer_vm::{TrapCode, VMOffsets};

/// A compiler that compiles a WebAssembly module with Singlepass.
/// It does the compilation in one pass
#[derive(MemoryUsage)]
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
}

impl Compiler for SinglepassCompiler {
    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    /// Compile the module using Singlepass, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        _module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError> {
        match target.triple().architecture {
            Architecture::X86_64 => {}
            Architecture::Aarch64(_) => {}
            _ => {
                return Err(CompileError::UnsupportedTarget(
                    target.triple().architecture.to_string(),
                ))
            }
        }
        if target.triple().architecture == Architecture::X86_64
            && !target.cpu_features().contains(CpuFeature::AVX)
        {
            return Err(CompileError::UnsupportedTarget(
                "x86_64 without AVX".to_string(),
            ));
        }
        if compile_info.features.multi_value {
            return Err(CompileError::UnsupportedFeature("multivalue".to_string()));
        }
        let calling_convention = match target.triple().default_calling_convention() {
            Ok(CallingConvention::WindowsFastcall) => CallingConvention::WindowsFastcall,
            Ok(CallingConvention::SystemV) => CallingConvention::SystemV,
            Ok(CallingConvention::AppleAarch64) => CallingConvention::AppleAarch64,
            _ => panic!("Unsupported Calling convention for Singlepass compiler"),
        };

        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let vmoffsets = VMOffsets::new(8, &compile_info.module);
        let module = &compile_info.module;
        let import_trampolines: PrimaryMap<SectionIndex, _> = (0..module.num_imported_functions)
            .map(FunctionIndex::new)
            .collect::<Vec<_>>()
            .into_par_iter_if_rayon()
            .map(|i| {
                gen_import_call_trampoline(
                    &vmoffsets,
                    i,
                    &module.signatures[module.functions[i]],
                    target,
                    calling_convention,
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect();
        let functions = function_body_inputs
            .iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .into_par_iter_if_rayon()
            .map(|(i, input)| {
                let middleware_chain = self
                    .config
                    .middlewares
                    .generate_function_middleware_chain(i);
                let mut reader =
                    MiddlewareBinaryReader::new_with_offset(input.data, input.module_offset);
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

                match target.triple().architecture {
                    Architecture::X86_64 => {
                        let machine = MachineX86_64::new();
                        let mut generator = FuncGen::new(
                            module,
                            &self.config,
                            &vmoffsets,
                            &memory_styles,
                            &table_styles,
                            i,
                            &locals,
                            machine,
                            calling_convention,
                        )
                        .map_err(to_compile_error)?;
                        while generator.has_control_frames() {
                            generator.set_srcloc(reader.original_position() as u32);
                            let op = reader.read_operator()?;
                            generator.feed_operator(op).map_err(to_compile_error)?;
                        }

                        Ok(generator.finalize(&input))
                    }
                    Architecture::Aarch64(_) => {
                        let machine = MachineARM64::new();
                        let mut generator = FuncGen::new(
                            module,
                            &self.config,
                            &vmoffsets,
                            &memory_styles,
                            &table_styles,
                            i,
                            &locals,
                            machine,
                            calling_convention,
                        )
                        .map_err(to_compile_error)?;
                        while generator.has_control_frames() {
                            generator.set_srcloc(reader.original_position() as u32);
                            let op = reader.read_operator()?;
                            generator.feed_operator(op).map_err(to_compile_error)?;
                        }

                        Ok(generator.finalize(&input))
                    }
                    _ => unimplemented!(),
                }
            })
            .collect::<Result<Vec<CompiledFunction>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<LocalFunctionIndex, CompiledFunction>>();

        let function_call_trampolines = module
            .signatures
            .values()
            .collect::<Vec<_>>()
            .into_par_iter_if_rayon()
            .map(|func_type| gen_std_trampoline(&func_type, target, calling_convention))
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<PrimaryMap<_, _>>();

        let dynamic_function_trampolines = module
            .imported_function_types()
            .collect::<Vec<_>>()
            .into_par_iter_if_rayon()
            .map(|func_type| {
                gen_std_dynamic_import_trampoline(
                    &vmoffsets,
                    &func_type,
                    target,
                    calling_convention,
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<PrimaryMap<FunctionIndex, FunctionBody>>();

        Ok(Compilation::new(
            functions,
            import_trampolines,
            function_call_trampolines,
            dynamic_function_trampolines,
            None,
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

trait IntoParIterIfRayon {
    type Output;
    fn into_par_iter_if_rayon(self) -> Self::Output;
}

impl<T: Send> IntoParIterIfRayon for Vec<T> {
    #[cfg(not(feature = "rayon"))]
    type Output = std::vec::IntoIter<T>;
    #[cfg(feature = "rayon")]
    type Output = rayon::vec::IntoIter<T>;

    fn into_par_iter_if_rayon(self) -> Self::Output {
        #[cfg(not(feature = "rayon"))]
        return self.into_iter();
        #[cfg(feature = "rayon")]
        return self.into_par_iter();
    }
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
            CompileError::UnsupportedTarget(name) => assert_eq!(name, "i686"), // Windows should be checked before architecture
            error => panic!("Unexpected error: {:?}", error),
        };
    }
}
