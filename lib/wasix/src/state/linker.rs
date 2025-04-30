// TODO: The linker *can* exist in the runtime, since technically, there's nothing that
// prevents us from having a non-WASIX linker. However, there is currently no use-case
// for a non-WASIX linker, so we'll refrain from making it generic for the time being.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use virtual_fs::{AsyncReadExt, FileSystem, FsError};
use virtual_mio::InlineWaker;
use wasmer::{
    AsStoreMut, CompileError, ExportError, Exportable, Extern, ExternType, Function, FunctionEnv,
    FunctionEnvMut, Global, GlobalType, ImportType, Imports, Instance, InstantiationError, Memory,
    MemoryError, Module, RuntimeError, Table, Type, Value, WASM_PAGE_SIZE,
};

use crate::{
    fs::WasiFsRoot, import_object_for_all_wasi_versions, ModuleInitializer, WasiEnv, WasiFs,
};

use super::WasiModuleInstanceHandles;

// Module handle 0 is always the main module. Side modules get handles starting from 1.
pub static MAIN_MODULE_HANDLE: ModuleHandle = ModuleHandle(0);

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

#[derive(thiserror::Error, Debug)]
pub enum MemoryDeallocationError {
    #[error("Invalid base address")]
    InvalidBaseAddress,
}

// Used to allocate and manage memory for dynamic modules that are loaded in or
// out, since each module may request a specific amount of memory to be allocated
// for it before starting it up.
struct MemoryAllocator {}

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

    // TODO: implement this
    pub fn deallocate(
        &mut self,
        _memory: &Memory,
        _store: &mut impl AsStoreMut,
        _addr: u64,
    ) -> Result<(), MemoryDeallocationError> {
        Ok(())
    }
}

pub struct DlModule {
    pub instance: Instance,
    pub memory_base: u64,
    pub table_base: u64,
    pub instance_handles: WasiModuleInstanceHandles,
    pub num_references: u32,
    _private: (),
}

#[derive(thiserror::Error, Debug)]
pub enum LinkError {
    #[error("Linker not initialized")]
    NotInitialized,

    // FIXME: support needed subsection, remove this error
    #[error("The 'needed' subsection of dylink.0 is not supported yet")]
    NeededSubsectionNotSupported,

    #[error("Main module is missing a required import: {0}")]
    MissingMainModuleImport(String),

    #[error("Module compilation error: {0}")]
    CompileError(#[from] CompileError),

    #[error("Failed to instantiate module: {0}")]
    InstantiationError(#[from] InstantiationError),

    #[error("Memory allocation error: {0}")]
    MemoryAllocationError(#[from] MemoryError),

    #[error("Runtime error: {0}")]
    TableAllocationError(RuntimeError),

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
    #[error("Linker not initialized")]
    NotInitialized,

    #[error("Invalid module handle")]
    InvalidModuleHandle,

    #[error("Missing export")]
    MissingExport,

    #[error("Invalid export type: {0:?}")]
    InvalidExportType(ExternType),
}

#[derive(thiserror::Error, Debug)]
pub enum UnloadError {
    #[error("Invalid module handle")]
    InvalidModuleHandle,

    #[error("Destructor function has invalid signature: {0}")]
    DtorFuncWithInvalidSignature(String),

    #[error("Destructor function {0} failed to run: {1}")]
    DtorFunctionFailed(String, RuntimeError),

    #[error("Failed to deallocate memory: {0}")]
    DeallocationError(#[from] MemoryDeallocationError),
}

pub struct DylinkInfo {
    pub mem_info: wasmparser::MemInfo,
}

pub struct LinkedMainModule {
    pub memory: Memory,
    pub indirect_function_table: Table,
    pub stack_low: u64,
    pub stack_high: u64,
}

/// The linker is responsible for loading and linking dynamic modules at runtime,
/// and managing the shared memory and indirect function table.
#[derive(Clone)]
pub struct Linker {
    state: Arc<Mutex<LinkerState>>,
}

struct LinkerState {
    main_instance: Option<Instance>,
    side_modules: HashMap<ModuleHandle, DlModule>,
    side_module_names: HashMap<PathBuf, ModuleHandle>,
    next_module_handle: u32,

    memory_allocator: MemoryAllocator,
    memory: Memory,

    stack_pointer: Global,
    #[allow(dead_code)]
    stack_high: u64,
    #[allow(dead_code)]
    stack_low: u64,

    // TODO: cache functions already placed in the table, so we don't add them again
    indirect_function_table: Table,
}

impl std::fmt::Debug for Linker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Linker").finish()
    }
}

impl Linker {
    // TODO: It makes more sense to move the entire instantiation flow here, but that requires a bigger
    // refactor in WasiEnv::instantiate. This will, however, remove the need for the initialize
    // function and the NotInitialized error, so it's a worthwhile refactor to make.

    /// Creates a new linker for the given main module. The module is expected to be a
    /// PIE executable. Imports for the module will be fulfilled, so that it can start
    /// running, and a Linker instance is returned which can then be used for the
    /// loading/linking of further side modules.
    /// Note, the returned linker must be initialized with the Instance corresponding
    /// to the main module by calling [`Linker::initialize`] before it can be used.
    pub fn new_for_main_module(
        main_module: &Module,
        store: &mut impl AsStoreMut,
        memory: Option<Memory>,
        imports: &mut Imports,
        stack_size: u64,
    ) -> Result<(Self, LinkedMainModule), LinkError> {
        let dylink_section = parse_dylink0_section(&main_module)?;

        let function_table_type = main_module
            .imports()
            .tables()
            .filter_map(|t| {
                if t.ty().ty == Type::FuncRef
                    && t.name() == "__indirect_function_table"
                    && t.module() == "env"
                {
                    Some(*t.ty())
                } else {
                    None
                }
            })
            .next()
            .ok_or(LinkError::MissingMainModuleImport(
                "env.__indirect_function_table".to_string(),
            ))?;

        let indirect_function_table = Table::new(store, function_table_type, Value::FuncRef(None))
            .map_err(LinkError::TableAllocationError)?;

        // Make sure the function table is as big as the dylink.0 section expects it to be
        if indirect_function_table.size(store) < dylink_section.mem_info.table_size as u32 {
            indirect_function_table
                .grow(
                    store,
                    dylink_section.mem_info.table_size as u32 - indirect_function_table.size(store),
                    Value::FuncRef(None),
                )
                .map_err(LinkError::TableAllocationError)?;
        }

        imports.define(
            "env",
            "__indirect_function_table",
            Extern::Table(indirect_function_table.clone()),
        );

        let memory_type = main_module
            .imports()
            .memories()
            .filter_map(|t| {
                if t.name() == "memory" && t.module() == "env" {
                    Some(*t.ty())
                } else {
                    None
                }
            })
            .next()
            .ok_or(LinkError::MissingMainModuleImport("env.memory".to_string()))?;

        let memory = match memory {
            Some(m) => m,
            None => Memory::new(store, memory_type)?,
        };

        let stack_low = {
            let data_end = dylink_section.mem_info.memory_size as u64;
            if data_end % 1024 != 0 {
                data_end + 1024 - (data_end % 1024)
            } else {
                data_end
            }
        };

        if stack_size % 1024 != 0 {
            panic!("Stack size must be 1024-bit aligned");
        }

        let stack_high = stack_low + stack_size;

        // Allocate memory for the stack. This does not need to go through the memory allocator
        // because:
        //   1. It's always placed directly after the module's data
        //   2. It's never freed, since the main module can't be unloaded
        memory.grow_at_least(store, stack_high)?;

        imports.define("env", "memory", Extern::Memory(memory.clone()));

        let mut stack_pointer = None;

        for import in main_module.imports() {
            match (import.module(), import.name()) {
                ("env", "__memory_base") => {
                    define_integer_global_import(store, imports, &import, 0)?;
                }
                ("env", "__table_base") => {
                    define_integer_global_import(store, imports, &import, 0)?;
                }
                ("env", "__stack_pointer") => {
                    stack_pointer = Some(define_integer_global_import(
                        store, imports, &import, stack_high,
                    )?);
                }
                ("GOT.mem", "__stack_high") => {
                    define_integer_global_import(store, imports, &import, stack_high)?;
                }
                ("GOT.mem", "__stack_low") => {
                    define_integer_global_import(store, imports, &import, stack_low)?;
                }
                ("GOT.mem", "__heap_base") => {
                    define_integer_global_import(store, imports, &import, stack_high)?;
                }
                _ => (),
            }
        }

        // We need the main module to import a stack pointer, so we can feed it to
        // the side modules later; thus, its absence is an error.
        let Some(stack_pointer) = stack_pointer else {
            return Err(LinkError::MissingMainModuleImport(
                "__stack_pointer".to_string(),
            ));
        };

        let linker = Self {
            state: Arc::new(Mutex::new(LinkerState {
                main_instance: None,
                side_modules: HashMap::new(),
                side_module_names: HashMap::new(),
                next_module_handle: 1,
                memory_allocator: MemoryAllocator::new(),
                memory: memory.clone(),
                stack_pointer,
                stack_high,
                stack_low,
                indirect_function_table: indirect_function_table.clone(),
            })),
        };

        Ok((
            linker,
            LinkedMainModule {
                memory,
                indirect_function_table,
                stack_low,
                stack_high,
            },
        ))
    }

    /// Initialize the linker with the instantiated main module. This needs to happen before the
    /// linker can be used to load any side modules.
    pub fn initialize(&self, main_instance: Instance) {
        let mut guard = self.state.lock().unwrap();
        guard.main_instance = Some(main_instance);
    }

    // TODO: figure out how this should work with threads...
    // TODO: auto-load dependencies, store relationship so dlsym can look inside deps of this lib as well
    // TODO: give loaded library a different wasi env that specifies its module handle
    // TODO: call destructors
    /// Loads a side module from the given path, linking it against the existing module tree
    /// and instantiating it. Symbols from the module can then be retrieved by calling
    /// [`Linker::resolve_export`].
    pub fn load_module(
        &self,
        module_path: impl AsRef<Path>,
        mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<ModuleHandle, LinkError> {
        {
            let mut guard = self.state.lock().unwrap();

            if let Some(handle) = guard.side_module_names.get(module_path.as_ref()) {
                let handle = *handle;
                let module = guard
                    .side_modules
                    .get_mut(&handle)
                    .expect("Internal error: side module names out of sync with side modules");
                module.num_references += 1;
                return Ok(handle);
            }
        }

        let func_env = ctx.as_ref().clone();

        let (env, mut store) = ctx.data_and_store_mut();
        let module_bytes =
            InlineWaker::block_on(locate_module(module_path.as_ref(), &env.state.fs))?;

        let module = Module::new(store.engine(), &*module_bytes)?;

        let dylink_info = parse_dylink0_section(&module)?;

        let mut guard = self.state.lock().unwrap();
        let memory = guard.memory.clone();

        let memory_base = guard.allocate_memory(&memory, &mut store, &dylink_info.mem_info)?;
        // TODO: handle table allocation... yes, we're even side-stepping that!
        let table_base = 0;

        let (imports, init) = guard.resolve_imports(
            &mut store,
            &func_env,
            &module,
            memory.clone(),
            memory_base,
            table_base,
        )?;

        let instance = Instance::new(&mut store, &module, &imports)?;

        let instance_handles =
            WasiModuleInstanceHandles::new(memory.clone(), &store, instance.clone());

        let loaded_module = DlModule {
            instance: instance.clone(),
            memory_base,
            table_base,
            instance_handles,
            num_references: 1,
            _private: (),
        };

        let handle = ModuleHandle(guard.next_module_handle);
        guard.next_module_handle += 1;

        guard.side_modules.insert(handle, loaded_module);
        guard
            .side_module_names
            .insert(module_path.as_ref().to_owned(), handle);

        let init = move || {
            // No idea at which point this should be called. Also, apparently, there isn't an actual
            // implementation of the init function that does anything (that I can find?), so it doesn't
            // matter anyway.
            init(&instance, &mut store).map_err(LinkError::InitializationError)?;

            call_initialization_function(&instance, &mut store, "__wasm_apply_data_relocs")?;
            call_initialization_function(&instance, &mut store, "__wasm_call_ctors")?;

            Ok(())
        };

        match init() {
            Ok(()) => Ok(handle),
            Err(e) => {
                guard.side_modules.remove(&handle);
                guard.side_module_names.remove(module_path.as_ref());
                Err(e)
            }
        }
    }

    pub fn unload_module(
        &self,
        module_handle: ModuleHandle,
        store: &mut impl AsStoreMut,
    ) -> Result<(), UnloadError> {
        let mut guard = self.state.lock().unwrap();

        let Some(module) = guard.side_modules.get_mut(&module_handle) else {
            return Err(UnloadError::InvalidModuleHandle);
        };

        module.num_references -= 1;

        // Module has more live references, so we're done
        if module.num_references > 0 {
            return Ok(());
        }

        // Otherwise, start actually unloading the module
        let module = guard.side_modules.remove(&module_handle).unwrap();
        guard
            .side_module_names
            .retain(|_, handle| *handle != module_handle);

        // TODO: need to add support for this in wasix-libc, currently it's not
        // exported from any side modules
        call_destructor_function(&module.instance, store, "__wasm_call_dtors")?;

        let memory = guard.memory.clone();
        guard
            .memory_allocator
            .deallocate(&memory, store, module.memory_base)?;

        // TODO: track holes in the function table as well?

        Ok(())
    }

    // TODO: Support RTLD_DEFAULT, RTLD_NEXT
    /// Resolves an export from the module corresponding to the given module handle.
    /// Only functions and globals can be resolved.
    ///
    /// If the symbol is a global, the returned value will be the absolute address of
    /// the data corresponding to that global within the shared linear memory.
    ///
    /// If it's a function, it'll be returned directly. The function can then be placed
    /// into the indirect function table by calling [`Linker::append_to_function_table`],
    /// which creates a "function pointer" that can be used from WASM code.
    pub fn resolve_export(
        &self,
        store: &mut impl AsStoreMut,
        module_handle: ModuleHandle,
        symbol: &str,
    ) -> Result<ResolvedExport, ResolveError> {
        let guard = self.state.lock().unwrap();
        let (instance, memory_base) = if module_handle == MAIN_MODULE_HANDLE {
            (
                guard
                    .main_instance()
                    .map_err(|()| ResolveError::NotInitialized)?,
                0,
            )
        } else {
            let module = guard
                .side_modules
                .get(&module_handle)
                .ok_or(ResolveError::InvalidModuleHandle)?;
            (&module.instance, module.memory_base)
        };

        let export = instance
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
                Ok(ResolvedExport::Global(value + memory_base))
            }
            ty => Err(ResolveError::InvalidExportType(ty.clone())),
        }
    }

    // TODO: cache functions so we don't grow the table unnecessarily?
    /// Places a function into the indirect function table, returning its index
    /// which can be given to WASM code as a function pointer.
    pub fn append_to_function_table(
        &self,
        store: &mut impl AsStoreMut,
        func: Function,
    ) -> Result<u32, LinkError> {
        let guard = self.state.lock().unwrap();
        let table = &guard.indirect_function_table;

        Ok(table
            .grow(store, 1, func.into())
            .map_err(LinkError::TableAllocationError)?)
    }

    /// Allows access to the internal representation of loaded modules. The modules
    /// can't be retrieved by reference because they live inside a mutex, so this
    /// function takes a callback and runs it on the module data instead.
    pub fn do_with_module<F, T>(&self, handle: ModuleHandle, callback: F) -> Option<T>
    where
        F: FnOnce(&DlModule) -> T,
    {
        let guard = self.state.lock().unwrap();
        let module = guard.side_modules.get(&handle)?;
        Some(callback(module))
    }
}

impl LinkerState {
    fn main_instance(&self) -> Result<&Instance, ()> {
        self.main_instance.as_ref().ok_or(())
    }

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
                "__indirect_function_table" => {
                    if !matches!(import.ty(), ExternType::Table(ty) if ty.ty == Type::FuncRef) {
                        return Err(LinkError::BadImport(
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    }
                    imports.define(
                        "env",
                        "__indirect_function_table",
                        Extern::Table(self.indirect_function_table.clone()),
                    );
                }
                "__stack_pointer" => {
                    if !matches!(import.ty(), ExternType::Global(ty) if *ty == self.stack_pointer.ty(store))
                    {
                        return Err(LinkError::BadImport(
                            import.name().to_string(),
                            import.ty().clone(),
                        ));
                    }
                    imports.define(
                        "env",
                        "__stack_pointer",
                        Extern::Global(self.stack_pointer.clone()),
                    );
                }
                "__memory_base" => {
                    define_integer_global_import(store, &mut imports, &import, memory_base)?;
                }
                "__table_base" => {
                    define_integer_global_import(store, &mut imports, &import, table_base)?;
                }
                name => {
                    // TODO: resolve symbols from other loaded modules as well
                    let Some(export) = self
                        .main_instance()
                        .map_err(|()| LinkError::NotInitialized)?
                        .exports
                        .get_extern(name)
                    else {
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

pub fn is_dynamically_linked(module: &Module) -> bool {
    module.custom_sections("dylink.0").next().is_some()
}

pub fn parse_dylink0_section(module: &Module) -> Result<DylinkInfo, LinkError> {
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

            wasmparser::Dylink0Subsection::Needed(_) => {
                return Err(LinkError::NeededSubsectionNotSupported)
            }

            // I haven't seen a single module with import or export info that's at least
            // consistent with its own imports/exports, so let's skip these
            wasmparser::Dylink0Subsection::ImportInfo(_)
            | wasmparser::Dylink0Subsection::ExportInfo(_)
            | wasmparser::Dylink0Subsection::Unknown { .. } => (),
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
) -> Result<Global, LinkError> {
    let ExternType::Global(GlobalType { ty, mutability }) = import.ty() else {
        return Err(LinkError::BadImport(
            import.name().to_string(),
            import.ty().clone(),
        ));
    };

    let new_global = if mutability.is_mutable() {
        Global::new_mut
    } else {
        Global::new
    };

    let global = match ty {
        Type::I32 => new_global(store, wasmer::Value::I32(value as i32)),
        Type::I64 => new_global(store, wasmer::Value::I64(value as i64)),
        _ => {
            return Err(LinkError::BadImport(
                import.name().to_string(),
                import.ty().clone(),
            ));
        }
    };

    imports.define(
        import.module(),
        import.name(),
        Extern::Global(global.clone()),
    );

    Ok(global)
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

fn call_destructor_function(
    instance: &Instance,
    store: &mut impl AsStoreMut,
    name: &str,
) -> Result<(), UnloadError> {
    match instance.exports.get_typed_function::<(), ()>(store, name) {
        Ok(f) => {
            f.call(store)
                .map_err(|e| UnloadError::DtorFunctionFailed(name.to_string(), e))?;
            Ok(())
        }
        Err(ExportError::Missing(_)) => Ok(()),
        Err(ExportError::IncompatibleType) => {
            Err(UnloadError::DtorFuncWithInvalidSignature(name.to_string()))
        }
    }
}
