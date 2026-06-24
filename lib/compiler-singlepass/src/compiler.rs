//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use crate::codegen::FuncGen;
use crate::config::{self, Singlepass};
use crate::machine::Machine;
use crate::machine::{
    gen_import_call_trampoline, gen_std_dynamic_import_trampoline, gen_std_trampoline,
};
use crate::machine_arm64::MachineARM64;
use crate::machine_riscv::MachineRiscv;
use crate::machine_x64::MachineX86_64;
use enumset::EnumSet;
use itertools::Itertools;
use object::write::{StandardSection, Symbol, SymbolSection};
use object::{SymbolFlags, SymbolKind, SymbolScope};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::{NamedTempFile, tempdir};
use wasmer_compiler::misc::{CompiledKind, save_assembly_to_file, types_to_signature};
use wasmer_compiler::object::get_object_for_target;
use wasmer_compiler::progress::ProgressContext;
use wasmer_compiler::{
    CompiledObjects, WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE, emit_metadata_and_link,
};
use wasmer_compiler::{
    Compiler, CompilerConfig, FunctionBinaryReader, FunctionBodyData, MiddlewareBinaryReader,
    ModuleMiddleware, ModuleMiddlewareChain, ModuleTranslationState,
    types::{
        function::{Compilation, CompiledFunction, FunctionBody, UnwindInfo},
        module::CompileModuleInfo,
        section::SectionIndex,
    },
};
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::target::{Architecture, CallingConvention, CpuFeature, Target};
use wasmer_types::{
    CompilationProgressCallback, CompileError, FunctionIndex, FunctionType, LocalFunctionIndex,
    MemoryIndex, ModuleInfo, TableIndex, TrapCode, TrapInformation, Type, VMOffsets,
};

/// A compiler that compiles a WebAssembly module with Singlepass.
/// It does the compilation in one pass
#[derive(Debug)]
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

    fn compile_module_internal(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        compile_info_blob: Vec<u8>,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<NamedTempFile, CompileError> {
        let arch = target.triple().architecture;
        match arch {
            Architecture::X86_64 => {}
            Architecture::Aarch64(_) => {}
            Architecture::Riscv64(_) => {}
            _ => {
                return Err(CompileError::UnsupportedTarget(
                    target.triple().architecture.to_string(),
                ));
            }
        };

        let calling_convention = match target.triple().default_calling_convention() {
            Ok(CallingConvention::WindowsFastcall) => CallingConvention::WindowsFastcall,
            Ok(CallingConvention::SystemV) => CallingConvention::SystemV,
            Ok(CallingConvention::AppleAarch64) => CallingConvention::AppleAarch64,
            _ => match target.triple().architecture {
                Architecture::Riscv64(_) => CallingConvention::SystemV,
                _ => {
                    return Err(CompileError::UnsupportedTarget(
                        "Unsupported Calling convention for Singlepass compiler".to_string(),
                    ));
                }
            },
        };

        let build_directory = tempdir().map_err(|err| {
            CompileError::Codegen(format!("cannot create temporary build folder: {err}"))
        })?;

        let module = &compile_info.module;
        let total_function_call_trampolines = module.signatures.len() as u64;
        let total_dynamic_trampolines = module.num_imported_functions as u64;
        let total_steps = WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE
            * ((total_dynamic_trampolines + total_function_call_trampolines) as u64)
            + function_body_inputs
                .iter()
                .map(|(_, body)| body.data.len() as u64)
                .sum::<u64>();
        let progress = progress_callback
            .cloned()
            .map(|cb| ProgressContext::new(cb, total_steps, "singlepass::functions"));

        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let vmoffsets = VMOffsets::new(8, &compile_info.module);
        let module = &compile_info.module;
        #[cfg_attr(not(feature = "unwind"), allow(unused_mut))]
        let mut custom_sections: PrimaryMap<SectionIndex, _> = (0..module.num_imported_functions)
            .map(FunctionIndex::new)
            .collect_vec()
            .into_par_iter()
            .map(|i| {
                gen_import_call_trampoline(
                    &vmoffsets,
                    i,
                    &module.signatures[module.functions[i]],
                    target,
                    calling_convention,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect();
        let object_files = function_body_inputs
            .iter()
            .collect_vec()
            .into_par_iter()
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

                let res = match arch {
                    Architecture::X86_64 => {
                        let machine = MachineX86_64::new(Some(target.clone()))?;
                        let mut generator = FuncGen::new(
                            module,
                            &self.config,
                            &vmoffsets,
                            memory_styles,
                            table_styles,
                            i,
                            &locals,
                            machine,
                            calling_convention,
                            target.triple(),
                            build_directory.path(),
                        )?;
                        while generator.has_control_frames() {
                            generator.set_srcloc(reader.original_position() as u32);
                            let op = reader.read_operator()?;
                            generator.feed_operator(op)?;
                        }

                        generator.finalize(input, arch)
                    }
                    Architecture::Aarch64(_) => {
                        let machine = MachineARM64::new(Some(target.clone()));
                        let mut generator = FuncGen::new(
                            module,
                            &self.config,
                            &vmoffsets,
                            memory_styles,
                            table_styles,
                            i,
                            &locals,
                            machine,
                            calling_convention,
                            target.triple(),
                            build_directory.path(),
                        )?;
                        while generator.has_control_frames() {
                            generator.set_srcloc(reader.original_position() as u32);
                            let op = reader.read_operator()?;
                            generator.feed_operator(op)?;
                        }

                        generator.finalize(input, arch)
                    }
                    Architecture::Riscv64(_) => {
                        let machine = MachineRiscv::new(
                            Some(target.clone()),
                            self.config.allow_experimental_unaligned_memory_accesses,
                        )?;
                        let mut generator = FuncGen::new(
                            module,
                            &self.config,
                            &vmoffsets,
                            memory_styles,
                            table_styles,
                            i,
                            &locals,
                            machine,
                            calling_convention,
                            target.triple(),
                            build_directory.path(),
                        )?;
                        while generator.has_control_frames() {
                            generator.set_srcloc(reader.original_position() as u32);
                            let op = reader.read_operator()?;
                            generator.feed_operator(op)?;
                        }

                        generator.finalize(input, arch)
                    }
                    _ => unimplemented!(),
                }?;

                if let Some(progress) = progress.as_ref() {
                    progress.notify_steps(input.data.len() as u64)?;
                }

                Ok(res)
            })
            .collect::<Result<Vec<_>, CompileError>>()?;

        let save_object = |obj: object::write::Object<'static>,
                           filename: String|
         -> Result<PathBuf, CompileError> {
            let object_path = build_directory.path().to_path_buf().join(&filename);
            let mut object_file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&object_path)
                .map_err(|e| {
                    CompileError::Codegen(format!("failed to create Wasmer object file: {e}"))
                })?;
            obj.write_stream(&mut object_file).map_err(|e| {
                CompileError::Codegen(format!("failed to write Wasmer object file: {e}"))
            })?;
            Ok(object_path)
        };

        let module_hash = module.hash_string();
        let trampolines_objects = module
            .signatures
            .iter()
            .collect_vec()
            .into_par_iter()
            .map(|(index, func_type)| -> Result<PathBuf, CompileError> {
                let body = gen_std_trampoline(func_type, target, calling_convention)?;
                if let Some(callbacks) = self.config.callbacks.as_ref() {
                    callbacks.obj_memory_buffer(
                        &CompiledKind::FunctionCallTrampoline(func_type.clone()),
                        &module_hash,
                        &body.body,
                    );
                    callbacks.asm_memory_buffer(
                        &CompiledKind::FunctionCallTrampoline(func_type.clone()),
                        &module_hash,
                        arch,
                        &body.body,
                        HashMap::new(),
                    )?;
                }
                if let Some(progress) = progress.as_ref() {
                    progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                }

                let mut obj = get_object_for_target(target.triple())
                    .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
                let symbol = obj.add_symbol(Symbol {
                    name: format!("t{}", index.as_u32()).into(),
                    value: 0,
                    size: body.body.len() as u64,
                    kind: SymbolKind::Text,
                    scope: SymbolScope::Dynamic,
                    weak: false,
                    section: SymbolSection::Undefined,
                    flags: SymbolFlags::None,
                });
                let text_section = obj.section_id(StandardSection::Text);
                obj.add_symbol_data(
                    symbol,
                    text_section,
                    &body.body,
                    // TODO
                    4,
                );

                save_object(obj, format!("t{}.o", index.as_u32()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let dynamic_functions_objects = module
            .imported_function_types()
            .enumerate()
            .collect_vec()
            .into_par_iter()
            .map(|(index, func_type)| -> Result<PathBuf, CompileError> {
                let body = gen_std_dynamic_import_trampoline(
                    &vmoffsets,
                    &func_type,
                    target,
                    calling_convention,
                )?;
                if let Some(callbacks) = self.config.callbacks.as_ref() {
                    callbacks.obj_memory_buffer(
                        &CompiledKind::DynamicFunctionTrampoline(func_type.clone()),
                        &module_hash,
                        &body.body,
                    );
                    callbacks.asm_memory_buffer(
                        &CompiledKind::DynamicFunctionTrampoline(func_type.clone()),
                        &module_hash,
                        arch,
                        &body.body,
                        HashMap::new(),
                    )?;
                }
                if let Some(progress) = progress.as_ref() {
                    progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                }

                let mut obj = get_object_for_target(target.triple())
                    .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
                let symbol = obj.add_symbol(Symbol {
                    name: format!("dt{index}").into(),
                    value: 0,
                    size: body.body.len() as u64,
                    kind: SymbolKind::Text,
                    scope: SymbolScope::Dynamic,
                    weak: false,
                    section: SymbolSection::Undefined,
                    flags: SymbolFlags::None,
                });
                let text_section = obj.section_id(StandardSection::Text);
                obj.add_symbol_data(
                    symbol,
                    text_section,
                    &body.body,
                    // TODO
                    4,
                );

                save_object(obj, format!("dt{index}.o"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // TODO: create temp file in caller
        let module_file = tempfile::Builder::new()
            .prefix("wasmer-image")
            .suffix(".so")
            .tempfile()
            .map_err(|err| CompileError::Codegen(format!("cannot create temporary file: {err}")))?;

        emit_metadata_and_link(
            target,
            compile_info_blob,
            build_directory.path(),
            module_file,
            &CompiledObjects {
                object_files: &object_files,
                trampoline_object_files: &trampolines_objects,
                dynamic_trampoline_object_files: &dynamic_functions_objects,
            },
            self.config
                .callbacks
                .as_ref()
                .map(|callbacks| callbacks.debug_dir.clone()),
            module_hash,
        )
    }
}

impl Compiler for SinglepassCompiler {
    fn name(&self) -> &str {
        "singlepass"
    }

    fn deterministic_id(&self) -> String {
        String::from("singlepass")
    }

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
        compile_info_blob: Vec<u8>,
        _module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<NamedTempFile, CompileError> {
        let num_threads = self.config.num_threads.get();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .map_err(|e| {
                CompileError::Codegen(format!("failed to build rayon thread pool: {e}"))
            })?;

        pool.install(|| {
            self.compile_module_internal(
                target,
                compile_info,
                compile_info_blob,
                function_body_inputs,
                progress_callback,
            )
        })
    }

    fn get_cpu_features_used(&self, cpu_features: &EnumSet<CpuFeature>) -> EnumSet<CpuFeature> {
        let used = CpuFeature::AVX | CpuFeature::SSE42 | CpuFeature::LZCNT | CpuFeature::BMI1;
        cpu_features.intersection(used)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use target_lexicon::triple;
    use wasmer_compiler::Features;
    use wasmer_types::{
        MemoryStyle, TableStyle,
        target::{CpuFeature, Triple},
    };

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
        let (info, translation, inputs) = dummy_compilation_ingredients();
        let result = compiler.compile_module(&linux32, &info, vec![], &translation, inputs, None);
        match result.unwrap_err() {
            CompileError::UnsupportedTarget(name) => assert_eq!(name, "i686"),
            error => panic!("Unexpected error: {error:?}"),
        };

        // Compile for win32
        let win32 = Target::new(triple!("i686-pc-windows-gnu"), CpuFeature::for_host());
        let (info, translation, inputs) = dummy_compilation_ingredients();
        let result = compiler.compile_module(&win32, &info, vec![], &translation, inputs, None);
        match result.unwrap_err() {
            CompileError::UnsupportedTarget(name) => assert_eq!(name, "i686"), // Windows should be checked before architecture
            error => panic!("Unexpected error: {error:?}"),
        };
    }

    #[test]
    fn errors_for_unsupported_cpufeatures() {
        let compiler = SinglepassCompiler::new(Singlepass::default());
        let mut features =
            CpuFeature::AVX | CpuFeature::SSE42 | CpuFeature::LZCNT | CpuFeature::BMI1;
        // simple test
        assert!(
            compiler.get_cpu_features_used(&features).is_subset(
                CpuFeature::AVX | CpuFeature::SSE42 | CpuFeature::LZCNT | CpuFeature::BMI1
            )
        );
        // check that an AVX build don't work on SSE4.2 only host
        assert!(
            !compiler
                .get_cpu_features_used(&features)
                .is_subset(CpuFeature::SSE42 | CpuFeature::LZCNT | CpuFeature::BMI1)
        );
        // check that having a host with AVX512 doesn't change anything
        features.insert_all(CpuFeature::AVX512DQ | CpuFeature::AVX512F);
        assert!(
            compiler.get_cpu_features_used(&features).is_subset(
                CpuFeature::AVX | CpuFeature::SSE42 | CpuFeature::LZCNT | CpuFeature::BMI1
            )
        );
    }
}
