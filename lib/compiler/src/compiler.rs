//! This module mainly outputs the `Compiler` trait that custom
//! compilers will need to implement.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::EH_FRAME_SECTION_NAME;
use crate::misc::{CompiledFunctionExt, CompiledKind};
use crate::object::get_object_for_target;
use crate::progress::ProgressContext;
use crate::types::function::Compilation;
use crate::types::module::CompileModuleInfo;
use crate::{
    FunctionBodyData, ModuleTranslationState, WASMER_FUNCTION_OFFSETS_SECTION_NAME,
    WASMER_TRAP_FUNCTION_OFFSETS_SECTION_NAME, translator::ModuleMiddleware,
};
use crossbeam_channel::unbounded;
use enumset::EnumSet;
use itertools::Itertools;
use libwild::{
    Args, FileSystem, FileType, InputFileData, Linker, OutputFileData, OutputOptions, error,
};
use object::write::{Relocation, StandardSegment, Symbol as ObjSymbol, SymbolSection};
use object::{
    RelocationEncoding, RelocationFlags, RelocationKind, SectionFlags, SectionKind, SymbolFlags,
    SymbolKind, SymbolScope, elf,
};
use std::{boxed::Box, sync::Arc};
use wasmer_types::{
    CompilationProgressCallback, Features, FunctionIndex, LocalFunctionIndex,
    entity::{EntityRef, PrimaryMap},
    error::CompileError,
    target::{CpuFeature, Target, UserCompilerOptimizations},
};
use wasmer_types::{FunctionType, SignatureIndex};
#[cfg(feature = "translator")]
use wasmparser::{Validator, WasmFeatures};

/// A debugger command-file format for registering JIT-compiled modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
pub enum Debugger {
    /// GDB command file.
    #[strum(serialize = "GDB")]
    Gdb,
    /// LLDB command file.
    #[strum(serialize = "LLDB")]
    Lldb,
}

/// A component representing a code-generation-sensitive aspect of a compiler
/// configuration for artifact format creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
#[allow(missing_docs)]
pub enum DeterministicIdComponent {
    #[strum(serialize = "llvm")]
    Llvm,
    #[strum(serialize = "cranelift")]
    Cranelift,
    #[strum(serialize = "singlepass")]
    Singlepass,
    #[strum(serialize = "opt0")]
    OptNone,
    #[strum(serialize = "optl")]
    OptLess,
    #[strum(serialize = "optd")]
    OptDefault,
    #[strum(serialize = "opta")]
    OptAggressive,
    #[strum(serialize = "opts")]
    OptSpeed,
    #[strum(serialize = "optsz")]
    OptSpeedAndSize,
    #[strum(serialize = "nan_canon")]
    NanCanonicalization,
    #[strum(serialize = "non_vol_mem")]
    NonVolatileMemops,
    #[strum(serialize = "pic")]
    Pic,
    #[strum(serialize = "ro_ftable")]
    ReadonlyFuncrefTable,
    #[strum(serialize = "unaligned_mem")]
    ExperimentalUnalignedMemoryAccesses,
}

/// The container format used for artifact serialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
pub enum ArtifactFormatContainer {
    /// rkyv serialization based.
    #[strum(serialize = "rkyv")]
    Rkyv,
    /// Native container, such as ELF.
    #[strum(serialize = "native")]
    Native,
}

/// The compiler configuration options.
pub trait CompilerConfig {
    /// Enable the experimental artifact format.
    fn experimental_artifact(&mut self, _enable: bool) {}

    /// Enable Position Independent Code (PIC).
    ///
    /// This is required for shared object generation (Native Engine),
    /// but will make the JIT Engine to fail, since PIC is not yet
    /// supported in the JIT linking phase.
    fn enable_pic(&mut self) {
        // By default we do nothing, each backend will need to customize this
        // in case they do something special for emitting PIC code.
    }

    /// Enable compiler IR verification.
    ///
    /// For compilers capable of doing so, this enables internal consistency
    /// checking.
    fn enable_verifier(&mut self) {
        // By default we do nothing, each backend will need to customize this
        // in case they create an IR that they can verify.
    }

    /// Enable generation of perfmaps to sample the JIT compiled frames.
    fn enable_perfmap(&mut self) {
        // By default we do nothing, each backend will need to customize this.
    }

    /// Enable generation of a debugger command file for JIT compiled frames.
    fn enable_debugger(&mut self, _debugger: Debugger) {
        // By default we do nothing, each backend will need to customize this.
    }

    /// For the LLVM compiler, we can use non-volatile memory operations which lead to a better performance
    /// (but are not 100% SPEC compliant).
    fn enable_non_volatile_memops(&mut self) {}

    /// Enable run-time handling of potentially unaligned memory accesses.
    ///
    /// This feature is experimental and currently supports only Cranelift scalar types
    /// and Singlepass on RISC-V for integral types.
    fn enable_experimental_unaligned_memory_accesses(&mut self) {}

    /// Enables treating eligible funcref tables as read-only so the backend can
    /// place them in read-only data.
    fn enable_readonly_funcref_table(&mut self) {}

    /// Enable NaN canonicalization.
    ///
    /// NaN canonicalization is useful when trying to run WebAssembly
    /// deterministically across different architectures.
    fn canonicalize_nans(&mut self, _enable: bool) {
        // By default we do nothing, each backend will need to customize this
        // in case they create an IR that they can verify.
    }

    /// Gets the custom compiler config
    fn compiler(self: Box<Self>) -> Box<dyn Compiler>;

    /// Gets the default features for this compiler in the given target
    fn default_features_for_target(&self, target: &Target) -> Features {
        self.supported_features_for_target(target)
    }

    /// Gets the supported features for this compiler in the given target
    fn supported_features_for_target(&self, _target: &Target) -> Features {
        Features::default()
    }

    /// Pushes a middleware onto the back of the middleware chain.
    fn push_middleware(&mut self, middleware: Arc<dyn ModuleMiddleware>);
}

impl<T> From<T> for Box<dyn CompilerConfig + 'static>
where
    T: CompilerConfig + 'static,
{
    fn from(other: T) -> Self {
        Box::new(other)
    }
}

/// An implementation of a Compiler from parsed WebAssembly module to Compiled native code.
pub trait Compiler: Send + std::fmt::Debug {
    /// Returns a descriptive name for this compiler.
    ///
    /// Note that this is an API breaking change since 3.0
    fn name(&self) -> &str;

    /// Returns the deterministic id of this compiler. Same compilers with different
    /// optimizations map to different deterministic IDs.
    fn deterministic_id(&self) -> String;

    /// Returns the used container for the artifact format: `rkyv` or `native`.
    fn container_format(&self) -> String {
        ArtifactFormatContainer::Rkyv.to_string()
    }

    /// Add suggested optimizations to this compiler.
    ///
    /// # Note
    ///
    /// Not every compiler supports every optimization. This function may fail (i.e. not set the
    /// suggested optimizations) silently if the underlying compiler does not support one or
    /// more optimizations.
    fn with_opts(
        &mut self,
        suggested_compiler_opts: &UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        _ = suggested_compiler_opts;
        Ok(())
    }

    /// Validates a module.
    ///
    /// It returns the a successful Result in case is valid, `CompileError` in case is not.
    #[cfg(feature = "translator")]
    fn validate_module(&self, features: &Features, data: &[u8]) -> Result<(), CompileError> {
        let mut wasm_features = WasmFeatures::empty();
        wasm_features.set(WasmFeatures::BULK_MEMORY, features.bulk_memory);
        wasm_features.set(WasmFeatures::THREADS, features.threads);
        wasm_features.set(WasmFeatures::REFERENCE_TYPES, features.reference_types);
        wasm_features.set(WasmFeatures::MULTI_VALUE, features.multi_value);
        wasm_features.set(WasmFeatures::SIMD, features.simd);
        wasm_features.set(WasmFeatures::TAIL_CALL, features.tail_call);
        wasm_features.set(WasmFeatures::MULTI_MEMORY, features.multi_memory);
        wasm_features.set(WasmFeatures::MEMORY64, features.memory64);
        wasm_features.set(WasmFeatures::EXCEPTIONS, features.exceptions);
        wasm_features.set(WasmFeatures::EXTENDED_CONST, features.extended_const);
        wasm_features.set(WasmFeatures::RELAXED_SIMD, features.relaxed_simd);
        wasm_features.set(WasmFeatures::WIDE_ARITHMETIC, features.wide_arithmetic);
        wasm_features.set(WasmFeatures::TAIL_CALL, features.tail_call);
        wasm_features.set(WasmFeatures::MUTABLE_GLOBAL, true);
        wasm_features.set(WasmFeatures::SATURATING_FLOAT_TO_INT, true);
        wasm_features.set(WasmFeatures::FLOATS, true);
        wasm_features.set(WasmFeatures::SIGN_EXTENSION, true);
        wasm_features.set(WasmFeatures::GC_TYPES, true);

        let mut validator = Validator::new_with_features(wasm_features);
        validator
            .validate_all(data)
            .map_err(|e| CompileError::Validate(format!("{e}")))?;
        Ok(())
    }

    /// Compiles a parsed module.
    ///
    /// It returns the [`Compilation`] or a [`CompileError`].
    fn compile_module(
        &self,
        target: &Target,
        module: &CompileModuleInfo,
        compile_info_blob: &[u8],
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<Compilation, CompileError>;

    /// Get the middlewares for this compiler
    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>];

    /// Get whether translation-time readonly funcref table analysis should run.
    fn enable_readonly_funcref_table(&self) -> bool {
        false
    }

    /// Get the CpuFeatures used by the compiler
    fn get_cpu_features_used(&self, cpu_features: &EnumSet<CpuFeature>) -> EnumSet<CpuFeature> {
        *cpu_features
    }

    /// Get whether `perfmap` is enabled or not.
    fn get_perfmap_enabled(&self) -> bool {
        false
    }

    /// Get the enabled debugger command-file format, if any.
    fn get_debugger(&self) -> Option<Debugger> {
        None
    }
}

/// A bucket containing a group of functions and their total size, used to balance compilation units for parallel compilation.
pub struct FunctionBucket<'a> {
    functions: Vec<(LocalFunctionIndex, &'a FunctionBodyData<'a>)>,
    /// IR size of the bucket (in bytes).
    pub size: usize,
}

impl<'a> FunctionBucket<'a> {
    /// Creates a new, empty `FunctionBucket`.
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            size: 0,
        }
    }
}

/// Build buckets sized by function length to keep compilation units balanced for parallel compilation.
pub fn build_function_buckets<'a>(
    function_body_inputs: &'a PrimaryMap<LocalFunctionIndex, FunctionBodyData<'a>>,
    bucket_threshold_size: u64,
) -> Vec<FunctionBucket<'a>> {
    let mut function_bodies = function_body_inputs
        .iter()
        .sorted_by_key(|(id, body)| Reverse((body.data.len(), id.as_u32())))
        .collect_vec();

    let mut buckets = Vec::new();

    while !function_bodies.is_empty() {
        let mut next_function_body = Vec::with_capacity(function_bodies.len());
        let mut bucket = FunctionBucket::new();

        for (fn_index, fn_body) in function_bodies.into_iter() {
            if bucket.size + fn_body.data.len() <= bucket_threshold_size as usize
                // Huge functions must fit into a bucket!
                || bucket.size == 0
            {
                bucket.size += fn_body.data.len();
                bucket.functions.push((fn_index, fn_body));
            } else {
                next_function_body.push((fn_index, fn_body));
            }
        }

        function_bodies = next_function_body;
        buckets.push(bucket);
    }

    buckets
}

/// Represents a function that has been compiled by the backend compiler.
pub trait CompiledFunction {}

/// Translates a function from its input representation to a compiled form.
pub trait FuncTranslator {}

/// Compile function buckets largest-first via the channel (instead of Rayon's par_iter).
#[allow(clippy::too_many_arguments)]
pub fn translate_function_buckets<'a, C, T, F, G>(
    pool: &rayon::ThreadPool,
    func_translator_builder: F,
    translate_fn: G,
    progress: Option<ProgressContext>,
    buckets: &[FunctionBucket<'a>],
) -> Result<Vec<C>, CompileError>
where
    T: FuncTranslator,
    C: CompiledFunction + Send + Sync,
    F: Fn() -> T + Send + Sync + Copy,
    G: Fn(&mut T, &LocalFunctionIndex, &FunctionBodyData) -> Result<C, CompileError>
        + Send
        + Sync
        + Copy,
{
    let progress = progress.as_ref();

    let functions = pool.install(|| {
        let (bucket_tx, bucket_rx) = unbounded::<&FunctionBucket<'a>>();
        for bucket in buckets {
            bucket_tx.send(bucket).map_err(|e| {
                CompileError::Resource(format!("cannot allocate crossbeam channel item: {e}"))
            })?;
        }
        drop(bucket_tx);

        let (result_tx, result_rx) =
            unbounded::<Result<Vec<(LocalFunctionIndex, C)>, CompileError>>();

        pool.scope(|s| {
            let worker_count = pool.current_num_threads().max(1);
            for _ in 0..worker_count {
                let bucket_rx = bucket_rx.clone();
                let result_tx = result_tx.clone();
                s.spawn(move |_| {
                    let mut func_translator = func_translator_builder();

                    while let Ok(bucket) = bucket_rx.recv() {
                        let bucket_result = (|| {
                            let mut translated_functions = Vec::new();
                            for (i, input) in bucket.functions.iter() {
                                let translated = translate_fn(&mut func_translator, i, input)?;
                                if let Some(progress) = progress {
                                    progress.notify_steps(input.data.len() as u64)?;
                                }
                                translated_functions.push((*i, translated));
                            }
                            Ok(translated_functions)
                        })();

                        if result_tx.send(bucket_result).is_err() {
                            break;
                        }
                    }
                });
            }
        });

        drop(result_tx);
        let mut functions = Vec::with_capacity(buckets.iter().map(|b| b.functions.len()).sum());
        for _ in 0..buckets.len() {
            match result_rx.recv().map_err(|e| {
                CompileError::Resource(format!("cannot allocate crossbeam channel item: {e}"))
            })? {
                Ok(bucket_functions) => functions.extend(bucket_functions),
                Err(err) => return Err(err),
            }
        }
        Ok(functions)
    })?;

    Ok(functions
        .into_iter()
        .sorted_by_key(|x| x.0)
        .map(|(_, body)| body)
        .collect_vec())
}

/// Byte size threshold for a function that is considered large.
pub const WASM_LARGE_FUNCTION_THRESHOLD: u64 = 100_000;

/// Estimated byte size of a trampoline (used for progress bar reporting).
pub const WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE: u64 = 1_000;

/// Holds the sets of compiled object buffers produced during compilation.
///
/// Counts of each category are derived from the slice lengths.
pub struct CompiledObjects {
    /// Objects for local (user-defined) functions.
    pub object_files: Vec<Vec<u8>>,
    /// Objects for imported function call trampolines.
    pub import_trampoline_object_files: Vec<Vec<u8>>,
    /// Objects for static trampolines.
    pub trampoline_object_files: Vec<Vec<u8>>,
    /// Objects for dynamic trampolines.
    pub dynamic_trampoline_object_files: Vec<Vec<u8>>,
}

fn emit_wasmer_meta_object(
    target: &Target,
    compile_info_blob: &[u8],
    compiled_objects: &CompiledObjects,
) -> Result<Vec<u8>, String> {
    let mut obj = get_object_for_target(target.triple())
        .map_err(|e| format!("failed to create Wasmer meta object: {e}"))?;

    let section_id = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        crate::WASMER_MODULE_INFO_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    obj.append_section_data(section_id, compile_info_blob, 8);
    obj.section_mut(section_id).flags = SectionFlags::Elf {
        sh_type: elf::SHT_PROGBITS,
        sh_flags: elf::SHF_GNU_RETAIN,
    };

    // Emit zero sentinel for the .eh_frame section.
    let section_id = obj.add_section(
        obj.segment_name(StandardSegment::Debug).to_vec(),
        EH_FRAME_SECTION_NAME.to_vec(),
        SectionKind::Debug,
    );
    obj.append_section_data(section_id, &0u64.to_ne_bytes(), 4);

    // Emit offsets of the functions
    let section_id = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        WASMER_FUNCTION_OFFSETS_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    obj.section_mut(section_id).flags = SectionFlags::Elf {
        sh_type: elf::SHT_PROGBITS,
        sh_flags: elf::SHF_GNU_RETAIN,
    };
    let pointer_size = target
        .triple()
        .pointer_width()
        .map_err(|_| "unknown pointer width".to_string())?
        .bytes() as u64;
    let pointer_bits = (pointer_size * 8) as u8;
    let zero_pointer = vec![0; pointer_size as usize];

    let function_offset_names = (0..compiled_objects.object_files.len())
        .map(|i| CompiledKind::Local(LocalFunctionIndex::new(i), String::new()).linkage_name())
        .chain(
            (0..compiled_objects.trampoline_object_files.len()).map(|i| {
                CompiledKind::FunctionCallTrampoline(
                    SignatureIndex::new(i),
                    // Unused by the linkage_name.
                    FunctionType::new([], []),
                )
                .linkage_name()
            }),
        )
        .chain(
            (0..compiled_objects.dynamic_trampoline_object_files.len()).map(|i| {
                CompiledKind::DynamicFunctionTrampoline(
                    FunctionIndex::new(i),
                    // Unused by the linkage_name.
                    FunctionType::new([], []),
                )
                .linkage_name()
            }),
        );
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

    let trap_fn_offsets_section_id = obj.add_section(
        obj.segment_name(StandardSegment::Data).to_vec(),
        WASMER_TRAP_FUNCTION_OFFSETS_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    obj.section_mut(trap_fn_offsets_section_id).flags = SectionFlags::Elf {
        sh_type: elf::SHT_PROGBITS,
        sh_flags: elf::SHF_GNU_RETAIN,
    };
    for traps_name in (0..compiled_objects.object_files.len())
        .map(|i| CompiledKind::Local(LocalFunctionIndex::new(i), String::new()).traps_name())
    {
        let offset =
            obj.append_section_data(trap_fn_offsets_section_id, &zero_pointer, pointer_size);
        let symbol_id = obj.add_symbol(ObjSymbol {
            name: traps_name.as_bytes().into(),
            value: 0,
            size: 0,
            kind: SymbolKind::Data,
            scope: SymbolScope::Linkage,
            weak: true,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        obj.add_relocation(
            trap_fn_offsets_section_id,
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
            format!("failed to add function trap offset relocation for {traps_name}: {e}")
        })?;
    }

    obj.write()
        .map_err(|e| format!("failed to serialize Wasmer meta object: {e}"))
}

#[derive(Clone, Default)]
struct InMemoryFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>>,
}

#[derive(Debug)]
struct InMemoryInput(Arc<Vec<u8>>);

impl InputFileData for InMemoryInput {
    fn bytes(&self) -> &[u8] {
        &self.0
    }
}

struct InMemoryOutput {
    path: PathBuf,
    bytes: Vec<u8>,
    files: Arc<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>>,
}

impl OutputFileData for InMemoryOutput {
    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
    fn finish(mut self) -> error::Result {
        self.files
            .lock()
            .map_err(|e| format!("cannot lock in-memory FS: {e}"))?
            .insert(self.path, Arc::new(std::mem::take(&mut self.bytes)));
        Ok(())
    }
}

impl FileSystem for InMemoryFileSystem {
    type Input = InMemoryInput;
    type Output = InMemoryOutput;

    fn open_input(&self, path: &Path, _: bool) -> error::Result<(Self::Input, Option<Arc<File>>)> {
        let bytes = self
            .files
            .lock()
            .map_err(|e| format!("cannot lock in-memory FS: {e}"))?
            .get(path)
            .map(Arc::clone)
            .ok_or_else(|| error!("No such in-memory file: {}", path.display()))?;
        Ok((InMemoryInput(bytes), None))
    }

    fn file_type(&self, path: &Path) -> error::Result<FileType> {
        self.files
            .lock()
            .map_err(|e| format!("cannot lock in-memory FS: {e}"))?
            .contains_key(path)
            .then_some(FileType::File)
            .ok_or_else(|| error!("no such in-memory file"))
    }

    fn canonicalize(&self, path: &Path) -> error::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
    fn remove_file(&self, path: &Path) -> error::Result<()> {
        self.files
            .lock()
            .map_err(|e| format!("cannot lock in-memory FS: {e}"))?
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| error!("no such in-memory file"))
    }
    fn rename_file(&self, path: &Path, new_path: &Path) -> error::Result<()> {
        let mut files = self
            .files
            .lock()
            .map_err(|e| format!("cannot lock in-memory FS: {e}"))?;
        let bytes = files
            .remove(path)
            .ok_or_else(|| error!("no such in-memory file"))?;
        files.insert(new_path.to_path_buf(), bytes);
        Ok(())
    }
    fn create_output(
        &self,
        path: Arc<Path>,
        options: OutputOptions,
    ) -> error::Result<Self::Output> {
        let size = usize::try_from(options.size).map_err(|_| error!("output is too large"))?;
        Ok(InMemoryOutput {
            path: path.to_path_buf(),
            bytes: vec![0; size],
            files: Arc::clone(&self.files),
        })
    }
    fn write_auxiliary(&self, path: &Path, bytes: &[u8]) -> error::Result {
        self.files
            .lock()
            .map_err(|e| format!("cannot lock in-memory FS: {e}"))?
            .insert(path.to_path_buf(), Arc::new(bytes.to_vec()));
        Ok(())
    }
}

const WASMER_IMAGE_FILENAME: &str = "wasmer-image.so";
const WASMER_META_FILENAME: &str = "__wasmer_meta.o";

/// Emits Wasmer metadata sections and links backend-generated object buffers into a shared object.
pub fn emit_metadata_and_link(
    pool: &rayon::ThreadPool,
    target: &Target,
    compile_info_blob: &[u8],
    compiled_objects: CompiledObjects,
    mut debug_dir: Option<PathBuf>,
    module_hash: Option<String>,
) -> Result<Vec<u8>, CompileError> {
    pool.install(|| {
        let meta_object = emit_wasmer_meta_object(target, compile_info_blob, &compiled_objects)
            .map_err(CompileError::Codegen)?;
        let CompiledObjects {
            object_files,
            import_trampoline_object_files,
            trampoline_object_files,
            dynamic_trampoline_object_files,
        } = compiled_objects;
        let fs = InMemoryFileSystem::default();
        let mut link_args = vec![
            "ld".to_string(),
            // Allow resolution of the public symbols directly without PLT entries!
            "-Bsymbolic".to_string(),
            "-shared".to_string(),
            "-z".to_string(),
            "now".to_string(),
            "-z".to_string(),
            "relro".to_string(),
            "-o".to_string(),
            WASMER_IMAGE_FILENAME.to_string(),
        ];

        {
            let mut files = fs
                .files
                .lock()
                .map_err(|e| CompileError::Codegen(format!("cannot lock in-memory FS: {e}")))?;
            for (index, object) in object_files
                .into_iter()
                .chain(import_trampoline_object_files)
                .chain(trampoline_object_files)
                .chain(dynamic_trampoline_object_files)
                .enumerate()
            {
                let path = PathBuf::from(format!("object-{index}.o"));
                files.insert(path.clone(), Arc::new(object));
                link_args.push(path.display().to_string());
            }
            files.insert(PathBuf::from(WASMER_META_FILENAME), Arc::new(meta_object));
        }
        // Keep the synthetic `.eh_frame` terminator after the real CIE/FDE
        // records. Linkers concatenate input sections in object order, and a
        // leading terminator makes frame registration see an empty table.
        link_args.push(WASMER_META_FILENAME.to_string());

        let mut wild_args = Args::new(|| link_args.iter().map(String::as_str)).map_err(|e| {
            CompileError::Codegen(format!("failed to initialize Wild linker: {e:?}"))
        })?;
        wild_args
            .parse(|| link_args.iter().map(String::as_str))
            .map_err(|e| {
                CompileError::Codegen(format!("failed to parse Wild linker args: {e:?}"))
            })?;
        Linker::with_file_system(fs.clone())
            .run(&wild_args)
            .map_err(|e| CompileError::Codegen(format!("Wild linker failed: {e:?}")))?;

        let image = fs
            .files
            .lock()
            .map_err(|e| CompileError::Codegen(format!("cannot lock in-memory FS: {e}")))?
            .remove(Path::new(WASMER_IMAGE_FILENAME))
            .ok_or_else(|| CompileError::Codegen("Wild linker did not produce an output".into()))?;
        let image = Arc::try_unwrap(image).map_err(|_| {
            CompileError::Codegen("Wild linker retained a reference to the output buffer".into())
        })?;

        // If compiler-debug-dir is set, copy the final linked .so image
        // into the module_hash subfolder.
        if let Some(debug_dir) = debug_dir.as_mut() {
            if let Some(ref hash) = module_hash {
                debug_dir.push(hash);
            }
            std::fs::create_dir_all(&debug_dir).ok();
            debug_dir.push(WASMER_IMAGE_FILENAME);
            let _ = std::fs::write(debug_dir, &image);
        }
        Ok(image)
    })
}
