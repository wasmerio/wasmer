use crate::config::LLVM;
use crate::config::OptimizationStyle;
use crate::translator::FuncTrampoline;
use crate::translator::FuncTranslator;
use object::Architecture;
use object::BinaryFormat;
use object::Endianness;
use object::SectionFlags;
use object::SectionKind;
use object::elf;
use object::write::Object;
use object::write::Relocation;
use object::write::StandardSegment;
use object::write::Symbol as ObjSymbol;
use object::write::SymbolSection;
use object::{
    RelocationEncoding, RelocationFlags, RelocationKind, SymbolFlags, SymbolKind, SymbolScope,
};
use rayon::ThreadPoolBuilder;
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::fs::OpenOptions;
use std::path::Path;
use std::path::PathBuf;
use std::{borrow::Cow, collections::HashMap, sync::Arc};
use wasmer_compiler::WASMER_FUNCTION_OFFSETS_SECTION_NAME;
use wasmer_compiler::WASMER_MODULE_INFO_SECTION_NAME;
use wasmer_compiler::WASMER_VERSION_SECTION_NAME;
use wasmer_compiler::progress::ProgressContext;
use wasmer_compiler::types::module::CompileModuleInfo;
use wasmer_compiler::{
    Compiler, FunctionBodyData, ModuleMiddleware, ModuleTranslationState,
    types::{
        section::SectionIndex,
        symbols::{Symbol, SymbolRegistry},
    },
};
use wasmer_compiler::{
    WASM_LARGE_FUNCTION_THRESHOLD, WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE, build_function_buckets,
    translate_function_buckets,
};
use wasmer_types::ExportIndex;
use wasmer_types::MetadataHeader;
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::target::Target;
use wasmer_types::{
    CompilationProgressCallback, CompileError, FunctionIndex, LocalFunctionIndex, ModuleInfo,
    SignatureIndex,
};

/// A compiler that compiles a WebAssembly module with LLVM, translating the Wasm to LLVM IR,
/// optimizing it and then translating to assembly.
#[derive(Debug)]
pub struct LLVMCompiler {
    config: LLVM,
}

impl LLVMCompiler {
    /// Creates a new LLVM compiler
    pub fn new(config: LLVM) -> LLVMCompiler {
        LLVMCompiler { config }
    }

    /// Gets the config for this Compiler
    fn config(&self) -> &LLVM {
        &self.config
    }
}

impl Compiler for LLVMCompiler {
    fn name(&self) -> &str {
        "llvm"
    }

    fn get_perfmap_enabled(&self) -> bool {
        self.config.enable_perfmap
    }

    fn deterministic_id(&self) -> String {
        format!(
            "llvm-{}",
            match self.config.opt_level {
                inkwell::OptimizationLevel::None => "opt0",
                inkwell::OptimizationLevel::Less => "optl",
                inkwell::OptimizationLevel::Default => "optd",
                inkwell::OptimizationLevel::Aggressive => "opta",
            }
        )
    }

    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    fn enable_readonly_funcref_table(&self) -> bool {
        self.config.enable_readonly_funcref_table
    }

    /// Compile the module using LLVM, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        compile_info_blob: Vec<u8>,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<(), CompileError> {
        let module = &compile_info.module;
        let module_hash = module.hash_string();

        let total_function_call_trampolines = module.signatures.len();
        let total_dynamic_trampolines = module.num_imported_functions;
        let total_steps = WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE
            * ((total_dynamic_trampolines + total_function_call_trampolines) as u64)
            + function_body_inputs
                .iter()
                .map(|(_, body)| body.data.len() as u64)
                .sum::<u64>();

        let progress = progress_callback
            .cloned()
            .map(|cb| ProgressContext::new(cb, total_steps, "Compiling functions"));

        let module = &compile_info.module;
        let memory_styles = &compile_info.memory_styles;
        let table_styles = &compile_info.table_styles;
        let signature_hashes = &module.signature_hashes;

        let build_directory = Path::new("/tmp/llvm-build");
        std::fs::create_dir_all(build_directory).map_err(|e| {
            CompileError::Codegen(format!(
                "failed to create LLVM build directory {}: {e}",
                build_directory.display()
            ))
        })?;

        let pool = ThreadPoolBuilder::new()
            .num_threads(self.config.num_threads.get())
            .build()
            .map_err(|e| CompileError::Resource(e.to_string()))?;

        let buckets =
            build_function_buckets(&function_body_inputs, WASM_LARGE_FUNCTION_THRESHOLD / 3);
        let largest_bucket = buckets.first().map(|b| b.size).unwrap_or_default();
        tracing::debug!(buckets = buckets.len(), largest_bucket, "buckets built");
        let object_files = translate_function_buckets(
            &pool,
            || {
                let compiler = &self;
                let target_machines = enum_iterator::all::<OptimizationStyle>()
                    .map(|style| {
                        (
                            style,
                            compiler.config().target_machine_with_opt(target, style),
                        )
                    })
                    .collect();
                let pointer_width = target.triple().pointer_width().unwrap().bytes();
                FuncTranslator::new(
                    target.triple().clone(),
                    target_machines,
                    pointer_width,
                    *target.cpu_features(),
                    self.config.enable_non_volatile_memops,
                    module
                        .exports
                        .get("__wasm_apply_data_relocs")
                        .and_then(|export| {
                            if let ExportIndex::Function(index) = export {
                                Some(*index)
                            } else {
                                None
                            }
                        }),
                )
                .unwrap()
            },
            |func_translator, i, input| {
                func_translator.translate(
                    module,
                    module_translation,
                    signature_hashes,
                    i,
                    input,
                    self.config(),
                    memory_styles,
                    table_styles,
                    target.triple(),
                    build_directory,
                )
            },
            progress.clone(),
            &buckets,
        )?;

        let progress = progress.clone();
        let trampolines_objects = pool.install(|| {
            module
                .signatures
                .iter()
                .collect::<Vec<_>>()
                .par_iter()
                .map_init(
                    || {
                        let target_machine = self.config().target_machine(target);
                        FuncTrampoline::new(target_machine, target.triple().clone()).unwrap()
                    },
                    |func_trampoline, (index, sig)| {
                        let function_name = format!("t{}", index.as_u32());
                        let trampoline = func_trampoline.trampoline(
                            sig,
                            self.config(),
                            &function_name,
                            compile_info,
                            build_directory,
                        );
                        if let Some(progress) = progress.as_ref() {
                            progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                        }
                        trampoline
                    },
                )
                .collect::<Result<Vec<_>, CompileError>>()
        })?;

        // TODO: I removed the parallel processing of dynamic trampolines because we're passing
        // the sections bytes and relocations directly into the trampoline generation function.
        // We can move that logic out and re-enable parallel processing. Hopefully, there aren't
        // enough dynamic trampolines to actually cause a noticeable performance degradation.
        let dynamic_trampolines_objects = {
            let progress = progress.clone();
            let target_machine = self.config().target_machine(target);
            let func_trampoline =
                FuncTrampoline::new(target_machine, target.triple().clone()).unwrap();
            module
                .imported_function_types()
                .collect::<Vec<_>>()
                .into_iter()
                .enumerate()
                .map(|(index, func_type)| {
                    let function_name = format!("dt{}", index);
                    let trampoline = func_trampoline.dynamic_trampoline(
                        &func_type,
                        self.config(),
                        &function_name,
                        index as u32,
                        &module_hash,
                        build_directory,
                    )?;
                    if let Some(progress) = progress.as_ref() {
                        progress.notify_steps(WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE)?;
                    }
                    Ok(trampoline)
                })
                .collect::<Result<Vec<_>, CompileError>>()
        }?;

        let meta_object_path = emit_wasmer_meta_object(
            target,
            compile_info_blob,
            build_directory,
            function_body_inputs.len(),
            trampolines_objects.len(),
            dynamic_trampolines_objects.len(),
        )
        .map_err(CompileError::Codegen)?;

        let image_path = build_directory.join("image.so");
        let mut link_args = vec![
            "ld".to_string(),
            "-Bsymbolic".to_string(),
            "--strip-all".to_string(),
            "-shared".to_string(),
            "-o".to_string(),
            image_path.display().to_string(),
            meta_object_path.display().to_string(),
        ];
        link_args.extend(
            object_files
                .iter()
                .chain(trampolines_objects.iter())
                .chain(dynamic_trampolines_objects.iter())
                .map(|path| path.display().to_string()),
        );
        let mut wild_args =
            libwild::Args::new(|| link_args.iter().map(String::as_str)).map_err(|e| {
                CompileError::Codegen(format!("failed to initialize Wild linker: {e:?}"))
            })?;
        wild_args
            .parse(|| link_args.iter().map(String::as_str))
            .map_err(|e| {
                CompileError::Codegen(format!("failed to parse Wild linker args: {e:?}"))
            })?;
        libwild::run(wild_args)
            .map_err(|e| CompileError::Codegen(format!("Wild linker failed: {e:?}")))?;

        Ok(())
    }

    fn with_opts(
        &mut self,
        _suggested_compiler_opts: &wasmer_types::target::UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        Ok(())
    }
}

fn emit_wasmer_meta_object(
    target: &Target,
    compile_info_blob: Vec<u8>,
    build_directory: &Path,
    functions_count: usize,
    trampolines_count: usize,
    dynamic_trampolines_count: usize,
) -> Result<PathBuf, String> {
    // TODO: document: Serialize ModuleInfo
    let meta_object_path = build_directory.to_path_buf().join("__wasmer_meta.o");
    let mut meta_object = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&meta_object_path)
        .map_err(|e| {
            format!(
                "failed to create Wasmer metaobject {}: {e}",
                meta_object_path.display()
            )
        })?;

    // TODO
    let mut obj = Object::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
    let section_id = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        WASMER_MODULE_INFO_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    obj.append_section_data(section_id, &compile_info_blob, 8);

    // ELF-only: mark section allocatable and retained by linker GC.
    obj.section_mut(section_id).flags = SectionFlags::Elf {
        sh_flags: u64::from(elf::SHF_GNU_RETAIN),
    };

    // Emit offsets of the functions
    let section_id = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        WASMER_FUNCTION_OFFSETS_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    obj.section_mut(section_id).flags = SectionFlags::Elf {
        sh_flags: u64::from(elf::SHF_GNU_RETAIN),
    };
    let pointer_size = target
        .triple()
        .pointer_width()
        .map_err(|_| "unknown pointer width".to_string())?
        .bytes() as u64;
    let pointer_bits = (pointer_size * 8) as u8;
    let zero_pointer = vec![0; pointer_size as usize];

    // We're using a fixed naming conventions for functions, trampolines and the dynamic trampolines:
    // f{number} for functions, t{number} for trampolines and dt{number} for dynamic trampolines.
    let function_offset_names = (0..functions_count)
        .map(|i| format!("f{i}"))
        .chain((0..trampolines_count).map(|i| format!("t{i}")))
        .chain((0..dynamic_trampolines_count).map(|i| format!("dt{i}")));
    for function_name in function_offset_names {
        let offset = obj.append_section_data(section_id, &zero_pointer, pointer_size);
        let symbol_id = obj.add_symbol(ObjSymbol {
            name: function_name.to_owned().into(),
            value: 0,
            size: 0,
            kind: SymbolKind::Text,
            scope: SymbolScope::Unknown,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        obj.add_relocation(
            section_id,
            Relocation {
                offset,
                flags: RelocationFlags::Generic {
                    kind: RelocationKind::Absolute,
                    encoding: RelocationEncoding::Generic,
                    size: pointer_bits,
                },
                symbol: symbol_id,
                addend: 0,
            },
        )
        .map_err(|e| {
            format!("failed to add function offset relocation for {function_name}: {e}")
        })?;
    }

    // Save artifact format version.
    let section_id = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        WASMER_VERSION_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    obj.section_mut(section_id).flags = SectionFlags::Elf {
        sh_flags: u64::from(elf::SHF_GNU_RETAIN),
    };
    obj.append_section_data(
        section_id,
        &MetadataHeader::CURRENT_VERSION.to_le_bytes(),
        pointer_size,
    );

    // Save the generated object file.
    obj.write_stream(&mut meta_object).map_err(|e| {
        format!(
            "failed to write Wasmer metaobject {}: {e}",
            meta_object_path.display(),
        )
    })?;

    Ok(meta_object_path)
}
