//! TODO: This module is placed here because it has to be moved to the runtime, so we don't want to keep
//! it well-organized enough to discourage that.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use virtual_fs::{AsyncReadExt, FileSystem, FsError};
use wasmer::{
    AsStoreMut, CompileError, ExportError, Exportable, Extern, ExternType, Function, FunctionEnv,
    FunctionEnvMut, Global, GlobalType, ImportType, Imports, Instance, InstantiationError, Memory,
    MemoryError, Module, RuntimeError, Type, Value, WASM_PAGE_SIZE,
};

use crate::{
    fs::WasiFsRoot, import_object_for_all_wasi_versions, ModuleInitializer, WasiEnv, WasiFs,
    WasiInstanceHandles,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleHandle(u32);

impl From<ModuleHandle> for u32 {
    fn from(handle: ModuleHandle) -> Self {
        handle.0
    }
}

impl From<u32> for ModuleHandle {
    fn from(handle: u32) -> Self {
        ModuleHandle(handle)
    }
}

const DEFAULT_RUNTIME_PATH: [&str; 3] = ["/lib", "/usr/lib", "/usr/local/lib"];

pub struct MemoryAllocator {}

impl MemoryAllocator {
    pub fn new() -> Self {
        Self {}
    }

    pub fn allocate(
        &mut self,
        memory: &Memory,
        store: &mut impl AsStoreMut,
        size: u64,
        _alignment: u32,
    ) -> Result<u64, MemoryError> {
        // TODO: no need to allocate entire pages of memory, but keeping it simple for now...
        // also, pages are already aligned, so no need to take the alignment into account
        let mut to_grow = size / WASM_PAGE_SIZE as u64;
        if size % WASM_PAGE_SIZE as u64 != 0 {
            to_grow += 1;
        }
        let pages = memory.grow(store, to_grow as u32)?;
        Ok(pages.0 as u64 * WASM_PAGE_SIZE as u64)
    }
}

struct DlModule {
    instance: Instance,
    memory_base: u64,
    table_base: u64,
}

struct LinkerState {
    main_module: Instance,
    side_modules: HashMap<ModuleHandle, DlModule>,
    side_module_names: HashMap<PathBuf, ModuleHandle>,
    memory_allocator: MemoryAllocator,
    next_module_handle: u32,
}

#[derive(Clone)]
pub struct Linker {
    state: Arc<Mutex<LinkerState>>,
}

#[derive(thiserror::Error, Debug)]
pub enum LinkError {
    #[error("Module compilation error: {0}")]
    CompileError(#[from] CompileError),

    #[error("Failed to instantiate module: {0}")]
    InstantiationError(#[from] InstantiationError),

    #[error("Memory allocation error: {0}")]
    MemoryAllocationError(#[from] MemoryError),

    #[error("File system error: {0}")]
    FileSystemError(#[from] FsError),

    #[error("Module is not a dynamic library")]
    NotDynamicLibrary,

    #[error("Failed to parse dylink.0 section: {0}")]
    Dylink0SectionParseError(#[from] wasmparser::BinaryReaderError),

    #[error("Bad known import: {0} of type {1:?}")]
    BadImport(String, ExternType),

    #[error("Import could not be satisfied because it's missing: {0}")]
    MissingImport(String),

    #[error(
        "Import could not be satisfied because of type mismatch: {0}, expected {1:?}, found {2:?}"
    )]
    ImportTypeMismatch(String, ExternType, ExternType),

    #[error("Failed to initialize instance: {0}")]
    InitializationError(anyhow::Error),

    #[error("Initialization function has invalid signature: {0}")]
    InitFuncWithInvalidSignature(String),

    #[error("Initialization function {0} failed to run: {1}")]
    InitFunctionFailed(String, RuntimeError),
}

pub enum ResolvedExport {
    Function(Function),

    // Contains the offset of the global in memory, with memory_base accounted for
    // See: https://github.com/WebAssembly/tool-conventions/blob/main/DynamicLinking.md#exports
    Global(u64),
}

#[derive(thiserror::Error, Debug)]
pub enum ResolveError {
    #[error("Invalid module handle")]
    InvalidModuleHandle,

    #[error("Missing export")]
    MissingExport,

    #[error("Invalid export type: {0:?}")]
    InvalidExportType(ExternType),
}

struct DylinkInfo {
    mem_info: wasmparser::MemInfo,
}

impl Linker {
    pub fn new(main_module: Instance) -> Self {
        Self {
            state: Arc::new(Mutex::new(LinkerState {
                main_module,
                side_modules: HashMap::new(),
                side_module_names: HashMap::new(),
                memory_allocator: MemoryAllocator::new(),
                next_module_handle: 1,
            })),
        }
    }

    // TODO: figure out how this should work with threads...
    // TODO: auto-load dependencies, store relationship so dlsym can look inside deps of this lib as well
    // TODO: give loaded library a different wasi env that specifies its module handle
    // TODO: add ref-counting for already-loaded modules, so dlclose can know when to actually close a module
    // TODO: call destructors
    pub async fn load_module(
        &self,
        module_path: impl AsRef<Path>,
        mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<ModuleHandle, LinkError> {
        {
            let guard = self.state.lock().unwrap();

            if let Some(handle) = guard.side_module_names.get(module_path.as_ref()) {
                debug_assert!(
                    guard.side_modules.contains_key(handle),
                    "Internal error: side module names out of sync with side modules"
                );
                return Ok(*handle);
            }
        }

        let (env, mut store) = ctx.data_and_store_mut();
        let module_bytes = locate_module(module_path.as_ref(), &env.state.fs).await?;

        let memory = unsafe { env.memory() }.clone();

        let module = Module::new(store.engine(), &*module_bytes)?;

        let dylink_info = parse_dylink0_section(&module)?;

        let mut guard = self.state.lock().unwrap();

        let memory_base = guard.allocate_memory(&memory, &mut store, &dylink_info.mem_info)?;
        // TODO: handle table allocation... yes, we're even side-stepping that!
        let table_base = 0;

        let func_env = wasmer::FunctionEnv::new(&mut store, env.clone());

        let (imports, init) = guard.resolve_imports(
            &mut store,
            &func_env,
            &module,
            memory.clone(),
            memory_base,
            table_base,
        )?;

        let instance = Instance::new(&mut store, &module, &imports)?;

        let wasi_handles = WasiInstanceHandles::new(memory, &store, instance.clone());
        func_env.as_mut(&mut store).set_inner(wasi_handles);

        // No idea at which point this should be called. Also, apparently, there isn't an actual
        // implementation of the init function that does anything (that I can find?), so it doesn't
        // matter anyway.
        init(&instance, &mut store).map_err(LinkError::InitializationError)?;

        call_initialization_function(&instance, &mut store, "__wasm_apply_data_relocs")?;
        call_initialization_function(&instance, &mut store, "__wasm_call_ctors")?;

        let loaded_module = DlModule {
            instance,
            memory_base,
            table_base,
        };

        let handle = ModuleHandle(guard.next_module_handle);
        guard.next_module_handle += 1;

        guard.side_modules.insert(handle, loaded_module);
        guard
            .side_module_names
            .insert(module_path.as_ref().to_owned(), handle);

        Ok(handle)
    }

    // TODO: Support RTLD_DEFAULT, RTLD_NEXT
    pub fn resolve_export(
        &self,
        store: &mut impl AsStoreMut,
        module_handle: ModuleHandle,
        symbol: &str,
    ) -> Result<ResolvedExport, ResolveError> {
        let guard = self.state.lock().unwrap();
        let module = guard
            .side_modules
            .get(&module_handle)
            .ok_or(ResolveError::InvalidModuleHandle)?;
        let export = module
            .instance
            .exports
            .get_extern(symbol)
            .ok_or(ResolveError::MissingExport)?;

        match export.ty(store) {
            ExternType::Function(_) => Ok(ResolvedExport::Function(
                Function::get_self_from_extern(export).unwrap().clone(),
            )),
            ty @ ExternType::Global(_) => {
                let global = Global::get_self_from_extern(export).unwrap();
                let value = match global.get(store) {
                    Value::I32(value) => value as u64,
                    Value::I64(value) => value as u64,
                    _ => return Err(ResolveError::InvalidExportType(ty.clone())),
                };
                Ok(ResolvedExport::Global(value + module.memory_base))
            }
            ty => Err(ResolveError::InvalidExportType(ty.clone())),
        }
    }
}

impl LinkerState {
    fn allocate_memory(
        &mut self,
        memory: &Memory,
        store: &mut impl AsStoreMut,
        mem_info: &wasmparser::MemInfo,
    ) -> Result<u64, MemoryError> {
        if mem_info.memory_size == 0 {
            Ok(0)
        } else {
            self.memory_allocator.allocate(
                memory,
                store,
                mem_info.memory_size as u64,
                2_u32.pow(mem_info.memory_alignment),
            )
        }
    }

    fn resolve_imports(
        &self,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        module: &Module,
        memory: Memory,
        memory_base: u64,
        table_base: u64,
    ) -> Result<(Imports, ModuleInitializer), LinkError> {
        let (mut imports, init) = import_object_for_all_wasi_versions(module, store, env);

        let mut memory = Some(memory);

        for import in module.imports() {
            // All DL-related imports are in the "env" module
            if import.module() != "env" {
                continue;
            }

            match import.name() {
                "memory" => {
                    if !matches!(import.ty(), ExternType::Memory(_)) {
                        return Err(LinkError::BadImport(
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    }
                    imports.define(
                        "env",
                        "memory",
                        Extern::Memory(memory.take().expect("env.memory imported multiple times")),
                    );
                }
                "__memory_base" => {
                    define_integer_global_import(store, &mut imports, &import, memory_base)?;
                }
                "__table_base" => {
                    define_integer_global_import(store, &mut imports, &import, table_base)?;
                }
                name => {
                    let Some(export) = self.main_module.exports.get_extern(name) else {
                        return Err(LinkError::MissingImport(name.to_string()));
                    };

                    let import_type = import.ty();
                    let export_type = export.ty(store);
                    if export_type != *import_type {
                        return Err(LinkError::ImportTypeMismatch(
                            name.to_string(),
                            import_type.clone(),
                            export_type,
                        ));
                    }

                    imports.define("env", name, export.clone());
                }
            }
        }

        Ok((imports, init))
    }
}

async fn locate_module(module_path: &Path, fs: &WasiFs) -> Result<Vec<u8>, LinkError> {
    async fn try_load(fs: &WasiFsRoot, path: impl AsRef<Path>) -> Result<Vec<u8>, FsError> {
        let mut file = match fs.new_open_options().read(true).open(path.as_ref()) {
            Ok(f) => f,
            // Fallback for cases where the module thinks it's running on unix,
            // but the compiled side module is a .wasm file
            Err(_) if path.as_ref().extension() == Some(OsStr::new("so")) => fs
                .new_open_options()
                .read(true)
                .open(path.as_ref().with_extension("wasm"))?,
            Err(e) => return Err(e),
        };

        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await?;
        Ok(buf)
    }

    if module_path.is_absolute() {
        Ok(try_load(&fs.root_fs, module_path).await?)
    } else if module_path.components().count() > 1 {
        Ok(try_load(
            &fs.root_fs,
            fs.relative_path_to_absolute(module_path.to_string_lossy().into_owned()),
        )
        .await?)
    } else {
        // Go through all dyanmic library lookup paths
        // TODO: implement RUNPATH
        // TODO: support $ORIGIN and ${ORIGIN} in RUNPATH

        // Note: a path without a slash does *not* look at the current directory.

        for path in DEFAULT_RUNTIME_PATH {
            if let Ok(module) = try_load(&fs.root_fs, Path::new(path).join(module_path)).await {
                return Ok(module);
            }
        }

        Err(FsError::EntryNotFound.into())
    }
}

fn parse_dylink0_section(module: &Module) -> Result<DylinkInfo, LinkError> {
    let mut sections = module.custom_sections("dylink.0");

    let Some(section) = sections.next() else {
        return Err(LinkError::NotDynamicLibrary);
    };

    // Verify the module contains exactly one dylink.0 section
    let None = sections.next() else {
        return Err(LinkError::NotDynamicLibrary);
    };

    let reader = wasmparser::Dylink0SectionReader::new(wasmparser::BinaryReader::new(&*section, 0));

    let mut mem_info = None;

    for subsection in reader {
        let subsection = subsection?;
        match subsection {
            wasmparser::Dylink0Subsection::MemInfo(m) => {
                mem_info = Some(m);
            }
            // TODO
            _ => todo!("handle other subsections"),
        }
    }

    Ok(DylinkInfo {
        mem_info: mem_info.unwrap_or_else(|| wasmparser::MemInfo {
            memory_size: 0,
            memory_alignment: 0,
            table_size: 0,
            table_alignment: 0,
        }),
    })
}

fn define_integer_global_import(
    store: &mut impl AsStoreMut,
    imports: &mut Imports,
    import: &ImportType,
    value: u64,
) -> Result<(), LinkError> {
    let ExternType::Global(GlobalType { ty, .. }) = import.ty() else {
        return Err(LinkError::BadImport(
            import.name().to_string(),
            import.ty().clone(),
        ));
    };
    match ty {
        Type::I32 => {
            imports.define(
                import.module(),
                import.name(),
                Extern::Global(Global::new(store, wasmer::Value::I32(value as i32))),
            );
        }
        Type::I64 => {
            imports.define(
                import.module(),
                import.name(),
                Extern::Global(Global::new(store, wasmer::Value::I64(value as i64))),
            );
        }
        _ => {
            return Err(LinkError::BadImport(
                import.name().to_string(),
                import.ty().clone(),
            ));
        }
    }

    Ok(())
}

fn call_initialization_function(
    instance: &Instance,
    store: &mut impl AsStoreMut,
    name: &str,
) -> Result<(), LinkError> {
    match instance.exports.get_typed_function::<(), ()>(store, name) {
        Ok(f) => {
            f.call(store)
                .map_err(|e| LinkError::InitFunctionFailed(name.to_string(), e))?;
            Ok(())
        }
        Err(ExportError::Missing(_)) => Ok(()),
        Err(ExportError::IncompatibleType) => {
            Err(LinkError::InitFuncWithInvalidSignature(name.to_string()))
        }
    }
}
