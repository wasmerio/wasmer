//! Define `NativeArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{NativeEngine, NativeEngineInner};
use crate::serialize::ModuleMetadata;
#[cfg(feature = "compiler")]
use faerie::{ArtifactBuilder, Decl, Link};
use libloading::{Library, Symbol};
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(feature = "compiler")]
use std::process::Command;
use std::sync::Arc;
use tempfile::NamedTempFile;
#[cfg(feature = "compiler")]
use tracing::trace;
use wasm_common::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasm_common::{
    FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
#[cfg(feature = "compiler")]
use wasmer_compiler::{BinaryFormat, ModuleEnvironment, RelocationTarget, Triple};
use wasmer_compiler::{CompileError, Features};
#[cfg(feature = "compiler")]
use wasmer_engine::Engine;
use wasmer_engine::{
    Artifact, DeserializeError, InstantiationError, LinkError, RuntimeError, SerializeError,
};
use wasmer_runtime::{MemoryPlan, TablePlan};
use wasmer_runtime::{ModuleInfo, VMFunctionBody, VMSharedSignatureIndex, VMTrampoline};

/// A compiled wasm module, ready to be instantiated.
pub struct NativeArtifact {
    sharedobject_path: PathBuf,
    metadata: ModuleMetadata,
    #[allow(dead_code)]
    library: Option<Library>,
    finished_functions: BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, *const VMFunctionBody>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
}

fn to_compile_error(err: impl Error) -> CompileError {
    CompileError::Codegen(format!("{}", err))
}

impl NativeArtifact {
    // Mach-O header in Mac
    #[allow(dead_code)]
    const MAGIC_HEADER_MH_CIGAM_64: &'static [u8] = &[207, 250, 237, 254];

    // ELF Magic header for Linux (32 bit)
    #[allow(dead_code)]
    const MAGIC_HEADER_ELF_32: &'static [u8] = &[0x7f, b'E', b'L', b'F', 1];

    // ELF Magic header for Linux (64 bit)
    #[allow(dead_code)]
    const MAGIC_HEADER_ELF_64: &'static [u8] = &[0x7f, b'E', b'L', b'F', 2];

    /// Check if the provided bytes look like `NativeArtifact`.
    ///
    /// This means, if the bytes look like a shared object file in the target
    /// system.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        cfg_if::cfg_if! {
            if #[cfg(all(target_pointer_width = "64", target_os="macos"))] {
                bytes.starts_with(Self::MAGIC_HEADER_MH_CIGAM_64)
            }
            else if #[cfg(all(target_pointer_width = "64", target_os="linux"))] {
                bytes.starts_with(Self::MAGIC_HEADER_ELF_64)
            }
            else if #[cfg(all(target_pointer_width = "32", target_os="linux"))] {
                bytes.starts_with(Self::MAGIC_HEADER_ELF_32)
            }
            else {
                false
            }
        }
    }

    /// Compile a data buffer into a `NativeArtifact`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(engine: &NativeEngine, data: &[u8]) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();
        let mut engine_inner = engine.inner_mut();
        let tunables = engine.tunables();

        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

        let memory_plans: PrimaryMap<MemoryIndex, MemoryPlan> = translation
            .module
            .memories
            .values()
            .map(|memory_type| tunables.memory_plan(*memory_type))
            .collect();
        let table_plans: PrimaryMap<TableIndex, TablePlan> = translation
            .module
            .tables
            .values()
            .map(|table_type| tunables.table_plan(*table_type))
            .collect();

        let compiler = engine_inner.compiler()?;

        // Compile the trampolines
        // We compile the call trampolines for all the signatures
        let func_types = translation
            .module
            .signatures
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let function_call_trampolines = compiler
            .compile_function_call_trampolines(&func_types)?
            .into_iter()
            .collect::<PrimaryMap<SignatureIndex, _>>();

        // We compile the dynamic function trampolines only for the imported functions
        let func_types = translation
            .module
            .functions
            .values()
            .take(translation.module.num_imported_funcs)
            .map(|&sig_index| translation.module.signatures[sig_index].clone())
            .collect::<Vec<_>>();
        let dynamic_function_trampolines =
            compiler.compile_dynamic_function_trampolines(&func_types)?;

        // Compile the Module
        let compilation = compiler.compile_module(
            &translation.module,
            translation.module_translation.as_ref().unwrap(),
            translation.function_body_inputs,
            memory_plans.clone(),
            table_plans.clone(),
        )?;

        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let target = compiler.target();
        let target_triple = target.triple().clone();
        if target_triple.binary_format != BinaryFormat::Elf
            && target_triple.binary_format != BinaryFormat::Macho
        {
            return Err(CompileError::Codegen(format!(
                "Binary format {} not supported",
                target_triple.binary_format
            )));
        }
        let mut obj = ArtifactBuilder::new(target_triple.clone())
            .name("module".to_string())
            .library(true)
            .finish();
        let function_bodies = compilation.get_function_bodies();

        // We construct the function body lengths
        let function_body_lengths = function_bodies
            .values()
            .map(|function_body| function_body.body.len() as u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();

        let metadata = ModuleMetadata {
            features: compiler.features().clone(),
            module: Arc::new(translation.module),
            prefix: engine_inner.get_prefix(&data),
            data_initializers,
            function_body_lengths,
            memory_plans,
            table_plans,
        };

        let serialized_data = bincode::serialize(&metadata).map_err(to_compile_error)?;

        obj.declare("WASMER_METADATA", Decl::data().global().read_only())
            .map_err(to_compile_error)?;

        let mut metadata_length = vec![0; 10];
        let mut writable = &mut metadata_length[..];
        leb128::write::unsigned(&mut writable, serialized_data.len() as u64)
            .expect("Should write number");
        metadata_length.extend(serialized_data);
        obj.define("WASMER_METADATA", metadata_length)
            .map_err(to_compile_error)?;

        let function_relocations = compilation.get_relocations();

        // Libcall declarations
        let libcalls = vec![
            "wasmer_f32_ceil",
            "wasmer_f32_floor",
            "wasmer_f32_trunc",
            "wasmer_f32_nearest",
            "wasmer_f64_ceil",
            "wasmer_f64_floor",
            "wasmer_f64_trunc",
            "wasmer_f64_nearest",
            "wasmer_raise_trap",
            "wasmer_probestack",
        ];
        for libcall in libcalls.iter() {
            obj.declare(libcall, Decl::function_import())
                .map_err(to_compile_error)?;
            obj.declare(&format!("_{}", libcall), Decl::function_import())
                .map_err(to_compile_error)?;
        }

        // Add functions
        for (function_local_index, function) in function_bodies.into_iter() {
            let function_name = Self::get_function_name(&metadata, function_local_index);
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add function call trampolines
        for (signature_index, function) in function_call_trampolines.into_iter() {
            let function_name = Self::get_function_call_trampoline_name(&metadata, signature_index);
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add dynamic function trampolines
        for (func_index, function) in dynamic_function_trampolines.into_iter() {
            let function_name = Self::get_dynamic_function_trampoline_name(&metadata, func_index);
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add function relocations
        for (function_local_index, function_relocs) in function_relocations.into_iter() {
            let function_name = Self::get_function_name(&metadata, function_local_index);
            for r in function_relocs {
                // assert_eq!(r.addend, 0);
                match r.reloc_target {
                    RelocationTarget::LocalFunc(index) => {
                        let target_name = Self::get_function_name(&metadata, index);
                        // println!("DOING RELOCATION offset: {} addend: {}", r.offset, r.addend);
                        obj.link(Link {
                            from: &function_name,
                            to: &target_name,
                            at: r.offset as u64,
                        })
                        .map_err(to_compile_error)?;
                    }
                    RelocationTarget::LibCall(libcall) => {
                        use wasmer_runtime::libcalls::LibCall;
                        let libcall_fn_name = match libcall {
                            LibCall::CeilF32 => "wasmer_f32_ceil",
                            LibCall::FloorF32 => "wasmer_f32_floor",
                            LibCall::TruncF32 => "wasmer_f32_trunc",
                            LibCall::NearestF32 => "wasmer_f32_nearest",
                            LibCall::CeilF64 => "wasmer_f64_ceil",
                            LibCall::FloorF64 => "wasmer_f64_floor",
                            LibCall::TruncF64 => "wasmer_f64_trunc",
                            LibCall::NearestF64 => "wasmer_f64_nearest",
                            LibCall::RaiseTrap => "wasmer_raise_trap",
                            LibCall::Probestack => "wasmer_probestack",
                        };
                        obj.link(Link {
                            from: &function_name,
                            to: &libcall_fn_name,
                            at: r.offset as u64,
                        })
                        .map_err(to_compile_error)?;
                    }
                    RelocationTarget::CustomSection(_custom_section) => {
                        // TODO: Implement custom sections
                    }
                    RelocationTarget::JumpTable(_func_index, _jt) => {
                        // do nothing
                    }
                };
            }
        }

        let file = NamedTempFile::new().map_err(to_compile_error)?;

        // Re-open it.
        let (file, filepath) = file.keep().map_err(to_compile_error)?;
        obj.write(file).map_err(to_compile_error)?;

        let shared_file = NamedTempFile::new().map_err(to_compile_error)?;
        let (_file, shared_filepath) = shared_file.keep().map_err(to_compile_error)?;
        let wasmer_symbols = libcalls
            .iter()
            .map(|libcall| {
                match target_triple.binary_format {
                    BinaryFormat::Macho => format!("-Wl,-U,_{}", libcall),
                    BinaryFormat::Elf => format!("-Wl,--undefined={}", libcall),
                    _ => {
                        // We should already be filtering only valid binary formats before
                        // so this should never happen.
                        unreachable!("Incorrect binary format")
                    }
                }
            })
            .collect::<Vec<String>>();

        let host_target = Triple::host();
        let is_cross_compiling = target_triple != host_target;
        let cross_compiling_args = if is_cross_compiling {
            vec![
                format!("--target={}", target_triple),
                "-fuse-ld=lld".to_string(),
                "-nodefaultlibs".to_string(),
                "-nostdlib".to_string(),
            ]
        } else {
            vec![]
        };
        trace!(
            "Compiling for target {} from host {}",
            target_triple.to_string(),
            host_target.to_string()
        );

        let linker = if is_cross_compiling {
            "clang-10"
        } else {
            "gcc"
        };

        let output = Command::new(linker)
            .arg(&filepath)
            .arg("-nostartfiles")
            .arg("-o")
            .arg(&shared_filepath)
            .arg("-Wl,-undefined,dynamic_lookup")
            // .args(&wasmer_symbols)
            .arg("-shared")
            .args(&cross_compiling_args)
            .arg("-v")
            .output()
            .map_err(to_compile_error)?;

        if !output.status.success() {
            return Err(CompileError::Codegen(format!(
                "Shared object file generator failed with:\n{}",
                std::str::from_utf8(&output.stderr).unwrap().trim_end()
            )));
        }
        trace!("gcc command result {:?}", output);
        if is_cross_compiling {
            Self::from_parts_crosscompiled(metadata, shared_filepath)
        } else {
            let lib = Library::new(&shared_filepath).map_err(to_compile_error)?;
            Self::from_parts(&mut engine_inner, metadata, shared_filepath, lib)
        }
    }

    fn get_function_name(metadata: &ModuleMetadata, index: LocalFunctionIndex) -> String {
        format!("wasmer_function_{}_{}", metadata.prefix, index.index())
    }

    fn get_function_call_trampoline_name(
        metadata: &ModuleMetadata,
        index: SignatureIndex,
    ) -> String {
        format!(
            "wasmer_trampoline_function_call_{}_{}",
            metadata.prefix,
            index.index()
        )
    }

    fn get_dynamic_function_trampoline_name(
        metadata: &ModuleMetadata,
        index: FunctionIndex,
    ) -> String {
        format!(
            "wasmer_trampoline_dynamic_function_{}_{}",
            metadata.prefix,
            index.index()
        )
    }

    /// Construct a `NativeArtifact` from component parts.
    pub fn from_parts_crosscompiled(
        metadata: ModuleMetadata,
        sharedobject_path: PathBuf,
    ) -> Result<Self, CompileError> {
        let finished_functions: PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]> =
            PrimaryMap::new();
        let finished_dynamic_function_trampolines: PrimaryMap<
            FunctionIndex,
            *const VMFunctionBody,
        > = PrimaryMap::new();
        let signatures: PrimaryMap<SignatureIndex, VMSharedSignatureIndex> = PrimaryMap::new();
        Ok(Self {
            sharedobject_path,
            metadata,
            library: None,
            finished_functions: finished_functions.into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
        })
    }

    /// Construct a `NativeArtifact` from component parts.
    pub fn from_parts(
        engine_inner: &mut NativeEngineInner,
        metadata: ModuleMetadata,
        sharedobject_path: PathBuf,
        lib: Library,
    ) -> Result<Self, CompileError> {
        let mut finished_functions: PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]> =
            PrimaryMap::new();
        for (function_local_index, function_len) in metadata.function_body_lengths.iter() {
            let function_name = Self::get_function_name(&metadata, function_local_index);
            unsafe {
                // We use a fake function signature `fn()` because we just
                // want to get the function address.
                let func: Symbol<unsafe extern "C" fn()> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                let raw = *func.into_raw();
                // The function pointer is a fat pointer, however this information
                // is only used when retrieving the trap information which is not yet
                // implemented in this engine.
                let func_pointer =
                    std::slice::from_raw_parts(raw as *const (), *function_len as usize);
                let func_pointer = func_pointer as *const [()] as *mut [VMFunctionBody];
                finished_functions.push(func_pointer);
            }
        }

        // Retrieve function call trampolines (for all signatures in the module)
        for (sig_index, func_type) in metadata.module.signatures.iter() {
            let function_name = Self::get_function_call_trampoline_name(&metadata, sig_index);
            unsafe {
                let trampoline: Symbol<VMTrampoline> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                engine_inner.add_trampoline(&func_type, *trampoline);
            }
        }

        // Retrieve dynamic function trampolines (only for imported functions)
        let mut finished_dynamic_function_trampolines: PrimaryMap<
            FunctionIndex,
            *const VMFunctionBody,
        > = PrimaryMap::with_capacity(metadata.module.num_imported_funcs);
        for func_index in metadata
            .module
            .functions
            .keys()
            .take(metadata.module.num_imported_funcs)
        {
            let function_name = Self::get_dynamic_function_trampoline_name(&metadata, func_index);
            unsafe {
                let trampoline: Symbol<*const VMFunctionBody> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                finished_dynamic_function_trampolines.push(*trampoline);
            }
        }

        // Leaving frame infos from now, as they are not yet used
        // however they might be useful for the future.
        // let frame_infos = compilation
        //     .get_frame_info()
        //     .values()
        //     .map(|frame_info| SerializableFunctionFrameInfo::Processed(frame_info.clone()))
        //     .collect::<PrimaryMap<LocalFunctionIndex, _>>();
        // Self::from_parts(&mut engine_inner, lib, metadata, )
        // let frame_info_registration = register_frame_info(
        //     serializable.module.clone(),
        //     &finished_functions,
        //     serializable.compilation.function_frame_info.clone(),
        // );

        // Compute indices into the shared signature table.
        let signatures = {
            let signature_registry = engine_inner.signatures();
            metadata
                .module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        Ok(Self {
            sharedobject_path,
            metadata,
            library: Some(lib),
            finished_functions: finished_functions.into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
        })
    }

    /// Compile a data buffer into a `NativeArtifact`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(_engine: &NativeEngine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Deserialize a `NativeArtifact` from bytes.
    ///
    /// # Safety
    ///
    /// The bytes must represent a serialized WebAssembly module.
    pub unsafe fn deserialize(
        engine: &NativeEngine,
        bytes: &[u8],
    ) -> Result<Self, DeserializeError> {
        if !Self::is_deserializable(&bytes) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not in any native format Wasmer can understand".to_string(),
            ));
        }
        // Dump the bytes into a file, so we can read it with our `dlopen`
        let named_file = NamedTempFile::new()?;
        let (mut file, path) = named_file.keep().map_err(|e| e.error)?;
        file.write_all(&bytes)?;
        // We already checked for the header, so we don't need
        // to check again.
        Self::deserialize_from_file_unchecked(&engine, &path)
    }

    /// Deserialize a `NativeArtifact` from a file path.
    ///
    /// # Safety
    ///
    /// The file's content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize_from_file(
        engine: &NativeEngine,
        path: &Path,
    ) -> Result<Self, DeserializeError> {
        let mut file = File::open(&path)?;
        let mut buffer = [0; 5];
        // read up to 5 bytes
        file.read_exact(&mut buffer)?;
        if !Self::is_deserializable(&buffer) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not in any native format Wasmer can understand".to_string(),
            ));
        }
        Self::deserialize_from_file_unchecked(&engine, &path)
    }

    /// Deserialize a `NativeArtifact` from a file path (unchecked).
    ///
    /// # Safety
    ///
    /// The file's content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize_from_file_unchecked(
        engine: &NativeEngine,
        path: &Path,
    ) -> Result<Self, DeserializeError> {
        let lib = Library::new(&path).map_err(|e| {
            DeserializeError::CorruptedBinary(format!("Library loading failed: {}", e))
        })?;
        let shared_path: PathBuf = PathBuf::from(path);
        // We use 10 + 1, as the length of the module will take 10 bytes
        // (we construct it like that in `metadata_length`) and we also want
        // to take the first element of the data to construct the slice from
        // it.
        let symbol: Symbol<*mut [u8; 10 + 1]> = lib.get(b"WASMER_METADATA").map_err(|e| {
            DeserializeError::CorruptedBinary(format!(
                "The provided object file doesn't seem to be generated by Wasmer: {}",
                e
            ))
        })?;
        use std::ops::Deref;
        use std::slice;

        let size = &mut **symbol.deref();
        let mut readable = &size[..];
        let metadata_len = leb128::read::unsigned(&mut readable).map_err(|_e| {
            DeserializeError::CorruptedBinary("Can't read metadata size".to_string())
        })?;
        let metadata_slice: &'static [u8] =
            slice::from_raw_parts(&size[10] as *const u8, metadata_len as usize);
        let metadata: ModuleMetadata = bincode::deserialize(metadata_slice)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        let mut engine_inner = engine.inner_mut();

        Self::from_parts(&mut engine_inner, metadata, shared_path, lib)
            .map_err(DeserializeError::Compiler)
    }
}

impl Artifact for NativeArtifact {
    fn module(&self) -> Arc<ModuleInfo> {
        self.metadata.module.clone()
    }

    fn module_ref(&self) -> &ModuleInfo {
        &self.metadata.module
    }

    fn module_mut(&mut self) -> Option<&mut ModuleInfo> {
        Arc::get_mut(&mut self.metadata.module)
    }

    fn register_frame_info(&self) {
        // Do nothing for now
    }

    fn features(&self) -> &Features {
        &self.metadata.features
    }

    fn data_initializers(&self) -> &[OwnedDataInitializer] {
        &*self.metadata.data_initializers
    }

    fn memory_plans(&self) -> &PrimaryMap<MemoryIndex, MemoryPlan> {
        &self.metadata.memory_plans
    }

    fn table_plans(&self) -> &PrimaryMap<TableIndex, TablePlan> {
        &self.metadata.table_plans
    }

    fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]> {
        &self.finished_functions
    }

    fn finished_dynamic_function_trampolines(
        &self,
    ) -> &BoxedSlice<FunctionIndex, *const VMFunctionBody> {
        &self.finished_dynamic_function_trampolines
    }

    fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self.signatures
    }

    fn preinstantiate(&self) -> Result<(), InstantiationError> {
        if self.library.is_none() {
            return Err(InstantiationError::Link(LinkError::Trap(
                RuntimeError::new("Cross compiled artifacts can't be instantiated."),
            )));
        }
        Ok(())
    }

    /// Serialize a NativeArtifact
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(std::fs::read(&self.sharedobject_path)?)
    }
}
