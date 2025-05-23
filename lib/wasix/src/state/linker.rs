// TODO: The linker *can* exist in the runtime, since technically, there's nothing that
// prevents us from having a non-WASIX linker. However, there is currently no use-case
// for a non-WASIX linker, so we'll refrain from making it generic for the time being.

//! Linker for loading and linking dynamic modules at runtime. The linker is designed to
//! work with output from clang (version 19 was used at the time of creating this code).
//! Note that dynamic linking of WASM modules is considered unstable in clang/LLVM, so
//! this code may need to be updated for future versions of clang.
//!
//! The linker doesn't care about where code exists and how modules call each other, but
//! the way we have found to be most effective is:
//!     * The main module carries with it all of wasix-libc, and exports everything
//!     * Side module don't link wasix-libc in, instead importing it from the main module
//! This way, we only need one instance of wasix-libc, and one instance of all the static
//! data that it requires to function. Indeed, if there were multiple instances of its
//! static data, it would more than likely just break completely; one needs only imagine
//! what would happen if there were multiple memory allocators (malloc) running at the same
//! time. Emscripten (the only WASM runtime that supports dynamic linking, at the time of
//! this writing) takes the same approach.
//!
//! While locating modules by relative or absolute paths is possible, it is recommended
//! to put every side module into /lib, where they can be located by name as well as by
//! path.
//!
//! The linker starts from a dynamically-linked main module. It scans the dylink.0 section
//! for memory and table-related information and the list of needed modules. The module
//! tree requires a memory, an indirect function table, and stack-related parameters
//! (including the __stack_pointer global), which are created. Since dynamically-linked
//! modules use PIC (position-independent code), the stack is not fixed and can be resized
//! at runtime.
//!
//! After the memory, function table and stack are created, the linker proceeds to load in
//! needed modules. Needed modules are always loaded in and initialized before modules that
//! asked for them, since it is expected that the needed module needs to be usable before
//! the module that needs it can be initialized.
//!
//! However, we also need to support circular dependencies between the modules; the most
//! common case is when the main needs a side module and imports function from it, and the
//! side imports wasix-libc functions from the main. To support this, the linker generates
//! stub functions for all the imports that cannot be resolved when a module is being
//! loaded in. The stub functions will then resolve the function once (and only once) at
//! runtime when they're first called. This *does*, however, mean that link error can happen
//! at runtime, after the linker has reported successful linking of the modules. Such errors
//! are turned into a [`WasiError::DlSymbolResolutionFailed`] error and will terminate
//! execution completely.
//!
//! The top-level overview of steps taken to link a main module is:
//!     * The main module is loaded in externally, at which point it is discovered that it
//!       is a dynamically-loaded module. This module is then passed in to
//!       [`Linker::new_for_main_module`].
//!     * The linker parses the dylink.0 section and creates a memory, function table and
//!       stack-related globals.
//!     * The linker loads in and instantiates all the needed modules, but does not initialize
//!       them yet. Imports for the main module are resolved, and control is returned to the
//!       calling code, which is expected to instantiate the main module, initialize the
//!       WasiEnv, and call [`Linker::initialize`].
//!     * In the [`Linker::initialize`] function, the link operation is "finalized": globals
//!       that couldn't be resolved due to circular dependencies are resolved to their
//!       correct values, and the init functions of all modules are run in LIFO order, so
//!       that the deepest needed module is initialized first.
//!     * After the call to [`Linker::initialize`] returns successfully, the module tree is
//!       now ready to be used and the main module's _start can be called.
//!
//! The top-level overview of steps taken to link a side module is:
//!     * The side module is located; Locating side modules happens as follows:
//!         * If the name contains a slash (/), it is treated as a relative or absolute path.   
//!         * Otherwise, the name is searched for in `/lib`, `/usr/lib` and `/usr/local/lib`.
//!       The same logic is applied to all needed side modules as well.
//!     * Once the module is located, the dylink.0 section is parsed. Memory is allocated for
//!       the module (see [`MemoryAllocator`]), as well as empty slots in the function table.
//!     * Needed modules are loaded in before the module itself is instantiated.
//!     * Once all modules are loaded in, the same link finalization steps are run: globals
//!       are resolved and init functions are run in LIFO order.
//!
//! Note that building modules that conform the specific requirements of this linker requires
//! careful configuration of clang. A PIC sysroot is required. The steps to build a main
//! module are:
//!
//! ```ignore
//! clang-19 \
//!   --target=wasm32-wasi --sysroot=/path/to/sysroot32-pic \
//!   -matomics -mbulk-memory -mmutable-globals -pthread \
//!   -mthread-model posix -ftls-model=local-exec \
//!   -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
//!   -D_WASI_EMULATED_PROCESS_CLOCKS \
//!   # PIC is required for all modules, main and side
//!   -fPIC \
//!   # We need to compile to an object file we can manually link in the next step
//!   -c main.c -o main.o
//!
//! wasm-ld-19 \
//!   # To link needed side modules, assuming `libsidewasm.so` exists in the current directory:
//!   -L. -lsidewasm \
//!   -L/path/to/sysroot32-pic/lib \
//!   -L/path/to/sysroot32-pic/lib/wasm32-wasi \
//!   # Make wasm-ld search everywhere and export everything, needed for wasix-libc functions to
//!   # be exported correctly from the main module
//!   --whole-archive --export-all \
//!   # The object file from the last step
//!   main.o \
//!   # The crt1.o file contains the _start and _main_void functions
//!   /path/to/sysroot32-pic/lib/wasm32-wasi/crt1.o \
//!   # Statically link the sysroot's libraries
//!   -lc -lresolv -lrt -lm -lpthread -lwasi-emulated-mman \
//!   # The usual linker config for wasix modules
//!   --import-memory --shared-memory --extra-features=atomics,bulk-memory,mutable-globals \
//!   --export=__wasm_signal --export=__tls_size --export=__tls_align \
//!   --export=__tls_base --export=__wasm_call_ctors --export-if-defined=__wasm_apply_data_relocs \
//!   # Again, PIC is very important, as well as producing a location-independent executable with -pie
//!   --experimental-pic -pie \
//!   -o main.wasm
//! ```
//!
//! And the steps to build a side module are:
//!
//! ```ignore
//! clang-19 \
//!   --target=wasm32-wasi --sysroot=/path/to/sysroot32-pic \
//!   -matomics -mbulk-memory -mmutable-globals -pthread \
//!   -mthread-model posix -ftls-model=local-exec \
//!   -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
//!   -D_WASI_EMULATED_PROCESS_CLOCKS \
//!   # We need PIC
//!   -fPIC
//!   # Make it export everything that's not hidden explicitly
//!   -fvisibility=default \
//!   -c side.c -o side.o
//!
//! wasm-ld-19 \
//!   # Note: we don't link against wasix-libc, so no -lc etc., because we want
//!   # those symbols to be imported.
//!   --extra-features=atomics,bulk-memory,mutable-globals \
//!   --export=__wasm_call_ctors --export-if-defined=__wasm_apply_data_relocs \
//!   # Need PIC
//!   --experimental-pic \
//!   # Import everything that's undefined, including wasix-libc functions
//!   --unresolved-symbols=import-dynamic \
//!   # build a shared library
//!   -shared \
//!   # Import a shared memory
//!   --shared-memory \
//!   # Conform to the libxxx.so naming so clang can find it via -lxxx
//!   -o libsidewasm.so side.o
//! ```

#![allow(clippy::result_large_err)]

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use virtual_fs::{AsyncReadExt, FileSystem, FsError};
use virtual_mio::InlineWaker;
use wasmer::{
    AsStoreMut, CompileError, ExportError, Exportable, Extern, ExternType, Function, FunctionEnv,
    FunctionEnvMut, FunctionType, Global, GlobalType, ImportType, Imports, Instance,
    InstantiationError, Memory, MemoryError, Module, RuntimeError, StoreMut, Table, Type, Value,
    WASM_PAGE_SIZE,
};
use wasmer_wasix_types::wasix::WasiMemoryLayout;

use crate::{
    fs::WasiFsRoot, import_object_for_all_wasi_versions, WasiEnv, WasiError, WasiFs,
    WasiFunctionEnv, WasiModuleTreeHandles,
};

use super::WasiModuleInstanceHandles;

// Module handle 0 is always the main module. Side modules get handles starting from 1.
pub static MAIN_MODULE_HANDLE: ModuleHandle = ModuleHandle(0);
static INVALID_MODULE_HANDLE: ModuleHandle = ModuleHandle(u32::MAX);

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

#[allow(clippy::manual_non_exhaustive)]
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

    #[error("Unresolved global '{0}'.{1} due to: {2}")]
    UnresolvedGlobal(String, String, ResolveError),

    #[error("Failed to update global {0} due to: {1}")]
    GlobalUpdateFailed(String, RuntimeError),

    #[error("Expected global to be of type I32 or I64: '{0}'.{1}")]
    NonIntegerGlobal(String, String),

    #[error("Bad known import: {0} of type {1:?}")]
    BadImport(String, String, ExternType),

    #[error(
        "Import could not be satisfied because of type mismatch: {0}, expected {1:?}, found {2:?}"
    )]
    ImportTypeMismatch(String, String, ExternType, ExternType),

    #[error("Expected import to be a function: env.{0}")]
    ImportMustBeFunction(String),

    #[error("Failed to initialize instance: {0}")]
    InitializationError(anyhow::Error),

    #[error("Initialization function has invalid signature: {0}")]
    InitFuncWithInvalidSignature(String),

    #[error("Initialization function {0} failed to run: {1}")]
    InitFunctionFailed(String, RuntimeError),

    #[error("Failed to initialize WASI(X) module handles: {0}")]
    MainModuleHandleInitFailed(ExportError),
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

#[derive(Debug)]
pub struct ExportInfo {
    pub name: String,
    pub flags: wasmparser::SymbolFlags,
}
impl From<&wasmparser::ExportInfo<'_>> for ExportInfo {
    fn from(info: &wasmparser::ExportInfo<'_>) -> Self {
        ExportInfo {
            name: info.name.to_string(),
            flags: info.flags,
        }
    }
}

#[derive(Debug)]
pub struct ImportInfo {
    pub module: String,
    pub field: String,
    pub flags: wasmparser::SymbolFlags,
}
impl From<&wasmparser::ImportInfo<'_>> for ImportInfo {
    fn from(info: &wasmparser::ImportInfo<'_>) -> Self {
        ImportInfo {
            module: info.module.to_string(),
            field: info.field.to_string(),
            flags: info.flags,
        }
    }
}

pub struct DylinkInfo {
    pub mem_info: wasmparser::MemInfo,
    pub needed: Vec<String>,
    pub import_info: Vec<ImportInfo>,
    pub export_info: Vec<ExportInfo>,
}

pub struct LinkedMainModule {
    pub instance: Instance,
    pub memory: Memory,
    pub indirect_function_table: Table,
    pub stack_low: u64,
    pub stack_high: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GlobalImportResolutionKind {
    Mem,
    Func,
}

impl GlobalImportResolutionKind {
    fn to_unresolved(self, name: String, global: Global) -> UnresolvedGlobal {
        match self {
            Self::Mem => UnresolvedGlobal::Mem(name, global),
            Self::Func => UnresolvedGlobal::Func(name, global),
        }
    }
}

enum UnresolvedGlobal {
    // A GOT.mem entry, should be resolved to an exported global from another module.
    Mem(String, Global),
    // A GOT.func entry, should be resolved to the address of an exported function
    // from another module (e.g. an index into __indirect_function_table).
    Func(String, Global),
}

type ModuleInitCallback = dyn FnOnce(&mut StoreMut) -> Result<(), LinkError>;

#[derive(Default)]
struct InProgressLinkState {
    // All modules loaded in by this link operation. Used to remove all the modules
    // in case a initializer function ends up failing to run.
    module_handles: Vec<ModuleHandle>,

    // List of all pending modules. We need this so we don't get stuck in an infinite
    // loop when modules have circular dependencies.
    pending_modules: Vec<PathBuf>,

    // List of globals we didn't manage to resolve yet. As the final step, this list
    // is iterated over and all globals filled in. If they still can't be resolved,
    // the entire link operation fails. This only works for mutable globals, but clang
    // appears to generate mutable globals for both GOT.mem and GOT.func.
    // The list contains the import types (name + global type), as well as the actual
    // (uninitialized) global we created for it.
    unresolved_globals: Vec<UnresolvedGlobal>,

    // List of modules for which init functions should be run. This needs to happen
    // as the last step, since the init functions may access stub globals or functions
    // which can't be resolved until the entire module tree is loaded in.
    init_callbacks: Vec<Box<ModuleInitCallback>>,
    reloc_callbacks: Vec<Box<ModuleInitCallback>>,
    ctor_callbacks: Vec<Box<ModuleInitCallback>>,
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

    indirect_function_table: Table,
}

impl std::fmt::Debug for Linker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Linker").finish()
    }
}

impl Linker {
    /// Creates a new linker for the given main module. The module is expected to be a
    /// PIE executable. Imports for the module will be fulfilled, so that it can start
    /// running, and a Linker instance is returned which can then be used for the
    /// loading/linking of further side modules.
    pub fn new(
        main_module: &Module,
        store: &mut StoreMut<'_>,
        memory: Option<Memory>,
        func_env: &mut WasiFunctionEnv,
        stack_size: u64,
    ) -> Result<(Self, LinkedMainModule), LinkError> {
        let dylink_section = parse_dylink0_section(main_module)?;

        let (mut imports, init_callback) =
            import_object_for_all_wasi_versions(main_module, store, &func_env.env);

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

        let stack_pointer_import = main_module
            .imports()
            .find(|i| i.module() == "env" && i.name() == "__stack_pointer")
            .ok_or(LinkError::MissingMainModuleImport(
                "__stack_pointer".to_string(),
            ))?;

        let stack_pointer = define_integer_global_import(store, &stack_pointer_import, stack_high)?;

        let mut linker_state = LinkerState {
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
        };

        let mut link_state = InProgressLinkState::default();

        for needed in dylink_section.needed {
            // A successful load_module will add the module to the side_modules list,
            // from which symbols can be resolved in the following call to
            // guard.resolve_imports.
            linker_state.load_module(needed, store, &func_env.env, &mut link_state)?;
        }

        linker_state.resolve_imports(
            store,
            &mut imports,
            &func_env.env,
            main_module,
            &mut link_state,
            &[
                ("env", "__memory_base", 0),
                ("env", "__table_base", 0),
                ("GOT.mem", "__stack_high", stack_high),
                ("GOT.mem", "__stack_low", stack_low),
                ("GOT.mem", "__heap_base", stack_high),
            ],
        )?;

        let main_instance = Instance::new(store, main_module, &imports)?;

        linker_state.main_instance = Some(main_instance.clone());

        let linker = Self {
            state: Arc::new(Mutex::new(linker_state)),
        };

        let stack_layout = WasiMemoryLayout {
            stack_lower: stack_low,
            stack_upper: stack_high,
            stack_size: stack_high - stack_low,
            guard_size: 0,
        };
        let module_handles = WasiModuleTreeHandles::Dynamic {
            linker: linker.clone(),
            main_module_instance_handles: WasiModuleInstanceHandles::new(
                memory.clone(),
                store,
                main_instance.clone(),
            ),
        };
        init_callback(&main_instance, store).map_err(LinkError::InitializationError)?;
        call_initialization_function(&main_instance, store, "__wasm_apply_data_relocs")?;

        func_env
            .initialize_handles_and_layout(
                store,
                main_instance.clone(),
                module_handles,
                Some(stack_layout),
                true,
            )
            .map_err(LinkError::MainModuleHandleInitFailed)?;

        {
            let guard = linker.state.lock().unwrap();
            linker.finalize_link_operation(guard, store, link_state)?;
        }


        // This function is exported from PIE executables, and needs to be run before calling
        // _initialize or _start. More info:
        // https://github.com/WebAssembly/tool-conventions/blob/main/DynamicLinking.md
        call_initialization_function(&main_instance, store, "_initialize")?;

        Ok((
            linker,
            LinkedMainModule {
                instance: main_instance,
                memory,
                indirect_function_table,
                stack_low,
                stack_high,
            },
        ))
    }

    /// Loads a side module from the given path, linking it against the existing module tree
    /// and instantiating it. Symbols from the module can then be retrieved by calling
    /// [`Linker::resolve_export`].
    pub fn load_module(
        &self,
        module_path: impl AsRef<Path>,
        mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<ModuleHandle, LinkError> {
        let mut guard = self.state.lock().unwrap();

        let mut link_state = InProgressLinkState::default();
        let env = ctx.as_ref();
        let mut store = ctx.as_store_mut();

        let module_handle = guard.load_module(module_path, &mut store, &env, &mut link_state)?;

        self.finalize_link_operation(guard, &mut store, link_state)?;

        Ok(module_handle)
    }

    fn finalize_link_operation(
        &self,
        // Take ownership of the guard and drop it ourselves to ensure no deadlock can happen
        guard: MutexGuard<LinkerState>,
        store: &mut impl AsStoreMut,
        mut link_state: InProgressLinkState,
    ) -> Result<(), LinkError> {
        // Can't have pending modules at this point now, can we?
        assert!(link_state.pending_modules.is_empty());

        guard.finalize_pending_globals(store, &link_state.unresolved_globals)?;

        // The linker must be unlocked for the next step, since modules may need to resolve
        // stub functions and that requires a lock on the linker's state
        drop(guard);

        self.run_initializers(store, &mut link_state)?;

        Ok(())
    }

    fn run_initializers(
        &self,
        store: &mut impl AsStoreMut,
        link_state: &mut InProgressLinkState,
    ) -> Result<(), LinkError> {
        let mut result = Ok(());

        let mut store_mut = store.as_store_mut();

        for init in link_state.init_callbacks.drain(..) {
            if let Err(e) = init(&mut store_mut) {
                result = Err(e);
                break;
            }
        }
        for init in link_state.reloc_callbacks.drain(..) {
            if let Err(e) = init(&mut store_mut) {
                result = Err(e);
                break;
            }
        }
        for init in link_state.ctor_callbacks.drain(..) {
            if let Err(e) = init(&mut store_mut) {
                result = Err(e);
                break;
            }
        }

        // If a module failed to load, the entire module tree is now invalid, so purge everything
        if result.is_err() {
            let mut guard = self.state.lock().unwrap();
            let memory = guard.memory.clone();

            for module_handle in link_state.module_handles.iter().cloned() {
                let module = guard.side_modules.remove(&module_handle).unwrap();
                guard
                    .side_module_names
                    .retain(|_, handle| *handle != module_handle);
                // We already have an error we need to report, so ignore memory deallocation errors
                _ = guard
                    .memory_allocator
                    .deallocate(&memory, store, module.memory_base);
            }
        }

        result
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
        // exported from any side modules. Each side module must have its own
        // __cxa_atexit and friends, and export its own __wasm_call_dtors.
        call_destructor_function(&module.instance, store, "__wasm_call_dtors")?;

        let memory = guard.memory.clone();
        guard
            .memory_allocator
            .deallocate(&memory, store, module.memory_base)?;

        // TODO: track holes in the function table as well?

        Ok(())
    }

    // TODO: Support RTLD_NEXT
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
        module_handle: Option<ModuleHandle>,
        symbol: &str,
    ) -> Result<ResolvedExport, ResolveError> {
        let guard = self.state.lock().unwrap();
        guard.resolve_export(store, module_handle, symbol)
    }

    /// Places a function into the indirect function table, returning its index
    /// which can be given to WASM code as a function pointer.
    pub fn append_to_function_table(
        &self,
        store: &mut impl AsStoreMut,
        func: Function,
    ) -> Result<u32, LinkError> {
        let guard = self.state.lock().unwrap();
        guard.append_to_function_table(store, func)
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
    fn main_instance(&self) -> Option<&Instance> {
        self.main_instance.as_ref()
    }

    fn allocate_memory(
        &mut self,
        store: &mut impl AsStoreMut,
        mem_info: &wasmparser::MemInfo,
    ) -> Result<u64, MemoryError> {
        if mem_info.memory_size == 0 {
            Ok(0)
        } else {
            self.memory_allocator.allocate(
                &self.memory,
                store,
                mem_info.memory_size as u64,
                2_u32.pow(mem_info.memory_alignment),
            )
        }
    }

    fn allocate_table(
        &mut self,
        store: &mut impl AsStoreMut,
        mem_info: &wasmparser::MemInfo,
    ) -> Result<u64, RuntimeError> {
        if mem_info.table_size == 0 {
            Ok(0)
        } else {
            let current_size = self.indirect_function_table.size(store);
            let alignment = 2_u32.pow(mem_info.table_alignment);

            let offset = if current_size % alignment != 0 {
                alignment - (current_size % alignment)
            } else {
                0
            };

            let start = self.indirect_function_table.grow(
                store,
                mem_info.table_size + offset,
                Value::FuncRef(None),
            )?;

            Ok((start + offset) as u64)
        }
    }

    fn resolve_imports(
        &self,
        store: &mut impl AsStoreMut,
        imports: &mut Imports,
        env: &FunctionEnv<WasiEnv>,
        module: &Module,
        link_state: &mut InProgressLinkState,
        well_known_imports: &[(&str, &str, u64)],
    ) -> Result<(), LinkError> {
        for import in module.imports() {
            if let Some(well_known_value) = well_known_imports.iter().find_map(|i| {
                if i.0 == import.module() && i.1 == import.name() {
                    Some(i.2)
                } else {
                    None
                }
            }) {
                imports.define(
                    import.module(),
                    import.name(),
                    define_integer_global_import(store, &import, well_known_value)?,
                );
            } else {
                match import.module() {
                    "env" => {
                        imports.define(
                            "env",
                            import.name(),
                            self.resolve_env_import(&import, store, env)?,
                        );
                    }
                    "GOT.mem" => {
                        imports.define(
                            "GOT.mem",
                            import.name(),
                            self.resolve_global_import(
                                &import,
                                store,
                                link_state,
                                GlobalImportResolutionKind::Mem,
                            )?,
                        );
                    }
                    "GOT.func" => {
                        imports.define(
                            "GOT.func",
                            import.name(),
                            self.resolve_global_import(
                                &import,
                                store,
                                link_state,
                                GlobalImportResolutionKind::Func,
                            )?,
                        );
                    }
                    _ => (),
                }
            }
        }

        Ok(())
    }

    // Imports from the env module are:
    //   * the memory and indirect function table
    //   * well-known addresses, such as __stack_pointer and __memory_base
    //   * functions that are imported directly
    fn resolve_env_import(
        &self,
        import: &ImportType,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
    ) -> Result<Extern, LinkError> {
        match import.name() {
            "memory" => {
                if !matches!(import.ty(), ExternType::Memory(_)) {
                    return Err(LinkError::BadImport(
                        import.module().to_string(),
                        import.name().to_string(),
                        import.ty().clone(),
                    ));
                }
                Ok(Extern::Memory(self.memory.clone()))
            }
            "__indirect_function_table" => {
                if !matches!(import.ty(), ExternType::Table(ty) if ty.ty == Type::FuncRef) {
                    return Err(LinkError::BadImport(
                        import.module().to_string(),
                        import.name().to_string(),
                        import.ty().clone(),
                    ));
                }
                Ok(Extern::Table(self.indirect_function_table.clone()))
            }
            "__stack_pointer" => {
                if !matches!(import.ty(), ExternType::Global(ty) if *ty == self.stack_pointer.ty(store))
                {
                    return Err(LinkError::BadImport(
                        import.module().to_string(),
                        import.name().to_string(),
                        import.ty().clone(),
                    ));
                }
                Ok(Extern::Global(self.stack_pointer.clone()))
            }
            name => {
                let export = self.resolve_symbol(name)?;

                match export {
                    Some(export) => {
                        let import_type = import.ty();
                        let export_type = export.ty(store);
                        if export_type != *import_type {
                            return Err(LinkError::ImportTypeMismatch(
                                "env".to_string(),
                                name.to_string(),
                                import_type.clone(),
                                export_type,
                            ));
                        }

                        Ok(export.clone())
                    }
                    None => {
                        // The function may be exported from a module we have yet to link in,
                        // or otherwise not be used by the module at all. We provide a stub that,
                        // when called, will try to resolve the symbol and call it. This lets
                        // us resolve circular dependencies, as well as letting modules that don't
                        // actually use their imports run successfully.
                        let ExternType::Function(func_ty) = import.ty() else {
                            return Err(LinkError::ImportMustBeFunction(name.to_string()));
                        };
                        Ok(self
                            .generate_stub_function(store, func_ty, env, name.to_string())
                            .into())
                    }
                }
            }
        }
    }

    // "Global" imports (i.e. imports from GOT.mem and GOT.func) are integer globals.
    // GOT.mem imports should point to the address of another module's data, while
    // GOT.func imports are function pointers (i.e. indices into the indirect function
    // table).
    fn resolve_global_import(
        &self,
        import: &ImportType,
        store: &mut impl AsStoreMut,
        link_state: &mut InProgressLinkState,
        global_kind: GlobalImportResolutionKind,
    ) -> Result<Global, LinkError> {
        let import_type = import.ty();
        let ExternType::Global(ty) = import_type else {
            return Err(LinkError::BadImport(
                import.module().to_owned(),
                import.name().to_owned(),
                import_type.clone(),
            ));
        };

        if !matches!(ty.ty, Type::I32 | Type::I64) {
            return Err(LinkError::NonIntegerGlobal(
                import.module().to_owned(),
                import.name().to_owned(),
            ));
        }

        let export = self.resolve_export(store, None, import.name());
        let (value, missing) = match export {
            Ok(ResolvedExport::Global(addr)) => {
                if global_kind == GlobalImportResolutionKind::Mem {
                    (addr, false)
                } else {
                    return Err(LinkError::UnresolvedGlobal(
                        import.module().to_owned(),
                        import.name().to_owned(),
                        ResolveError::MissingExport,
                    ));
                }
            }
            Ok(ResolvedExport::Function(func)) => {
                if global_kind == GlobalImportResolutionKind::Func {
                    let func_handle = self.append_to_function_table(store, func)?;
                    (func_handle as u64, false)
                } else {
                    return Err(LinkError::UnresolvedGlobal(
                        import.module().to_owned(),
                        import.name().to_owned(),
                        ResolveError::MissingExport,
                    ));
                }
            }
            Err(ResolveError::MissingExport) => (0, true),
            Err(e) => {
                return Err(LinkError::UnresolvedGlobal(
                    import.module().to_owned(),
                    import.name().to_owned(),
                    e,
                ))
            }
        };

        let global = define_integer_global_import(store, import, value)?;

        // if missing {
        //     link_state
        //         .unresolved_globals
        //         .push(global_kind.to_unresolved(import.name().to_owned(), global.clone()));
        // }

        Ok(global)
    }

    fn resolve_symbol(&self, symbol: &str) -> Result<Option<&Extern>, LinkError> {
        if let Some(export) = self
            .main_instance()
            .and_then(|instance| instance.exports.get_extern(symbol))
        {
            Ok(Some(export))
        } else {
            for module in self.side_modules.values() {
                if let Some(export) = module.instance.exports.get_extern(symbol) {
                    return Ok(Some(export));
                }
            }

            Ok(None)
        }
    }

    fn generate_stub_function(
        &self,
        store: &mut impl AsStoreMut,
        ty: &FunctionType,
        env: &FunctionEnv<WasiEnv>,
        name: String,
    ) -> Function {
        // TODO: since the instances are kept in the linker, and they can have stub functions,
        // and the stub functions reference the linker with a strong pointer, this probably
        // creates a cycle and memory leak. We need to use weak pointers here if that is the case.
        let ty = ty.clone();
        let resolved: Mutex<Option<Option<Function>>> = Mutex::new(None);
        Function::new_with_env(
            store,
            env,
            ty.clone(),
            move |mut env: FunctionEnvMut<'_, WasiEnv>, params: &[Value]| {
                let mk_error = || {
                    RuntimeError::user(Box::new(WasiError::DlSymbolResolutionFailed(name.clone())))
                };

                let mut resolved_guard = resolved.lock().unwrap();
                let func = match *resolved_guard {
                    None => {
                        let (data, store) = env.data_and_store_mut();
                        let env_inner = data.inner();
                        // Safe to unwrap since we already know we're doing DL
                        let linker = env_inner.linker().unwrap().clone();

                        let state_guard = linker.state.lock().unwrap();
                        let export = state_guard
                            .resolve_symbol(name.as_str())
                            .map_err(|_| mk_error())?;
                        let Some(export) = export else {
                            *resolved_guard = Some(None);
                            return Err(mk_error());
                        };
                        let Extern::Function(func) = export else {
                            *resolved_guard = Some(None);
                            return Err(mk_error());
                        };
                        if func.ty(&store) != ty {
                            *resolved_guard = Some(None);
                            return Err(mk_error());
                        }
                        *resolved_guard = Some(Some(func.clone()));
                        func.clone()
                    }
                    Some(None) => return Err(mk_error()),
                    Some(Some(ref func)) => func.clone(),
                };
                drop(resolved_guard);

                let mut store = env.as_store_mut();
                func.call(&mut store, params).map(|ret| ret.into())
            },
        )
    }

    // TODO: figure out how this should work with threads...
    // TODO: give loaded library a different wasi env that specifies its module handle
    fn load_module(
        &mut self,
        module_path: impl AsRef<Path>,
        store: &mut StoreMut<'_>,
        env: &FunctionEnv<WasiEnv>,
        link_state: &mut InProgressLinkState,
    ) -> Result<ModuleHandle, LinkError> {
        {
            if let Some(handle) = self.side_module_names.get(module_path.as_ref()) {
                let handle = *handle;
                let module = self
                    .side_modules
                    .get_mut(&handle)
                    .expect("Internal error: side module names out of sync with side modules");
                module.num_references += 1;
                return Ok(handle);
            }
        }

        let (full_path, module_bytes) = InlineWaker::block_on(locate_module(
            module_path.as_ref(),
            &env.as_ref(store).state.fs,
        ))?;

        // TODO: this can be optimized by detecting early if the module is already
        // pending without loading its bytes
        if link_state.pending_modules.contains(&full_path) {
            // This is fine, since a non-empty pending_modules list means we are
            // recursively resolving needed modules. We don't use the handle
            // returned from this function for anything when running recursively
            // (see self.load_module call below).
            return Ok(INVALID_MODULE_HANDLE);
        }

        let module = Module::new(store.engine(), &*module_bytes)?;

        let dylink_info = parse_dylink0_section(&module)?;

        link_state.pending_modules.push(full_path);
        let num_pending_modules = link_state.pending_modules.len();
        let pop_pending_module = |link_state: &mut InProgressLinkState| {
            assert_eq!(
                num_pending_modules,
                link_state.pending_modules.len(),
                "Internal error: pending modules not maintained correctly"
            );
            link_state.pending_modules.pop().unwrap();
        };

        for needed in dylink_info.needed {
            // A successful load_module will add the module to the side_modules list,
            // from which symbols can be resolved in the following call to
            // self.resolve_imports.
            match self.load_module(needed, store, env, link_state) {
                Ok(_) => (),
                Err(e) => {
                    pop_pending_module(link_state);
                    return Err(e);
                }
            }
        }

        pop_pending_module(link_state);

        let memory_base = self.allocate_memory(store, &dylink_info.mem_info)?;
        let table_base = self
            .allocate_table(store, &dylink_info.mem_info)
            .map_err(LinkError::TableAllocationError)?;

        let (mut imports, init) = import_object_for_all_wasi_versions(&module, store, env);

        self.resolve_imports(
            store,
            &mut imports,
            env,
            &module,
            link_state,
            &[
                ("env", "__memory_base", memory_base),
                ("env", "__table_base", table_base),
            ],
        )?;

        let instance = Instance::new(store, &module, &imports)?;

        let instance_handles =
            WasiModuleInstanceHandles::new(self.memory.clone(), store, instance.clone());

        let loaded_module = DlModule {
            instance: instance.clone(),
            memory_base,
            table_base,
            instance_handles,
            num_references: 1,
            _private: (),
        };

        let handle = ModuleHandle(self.next_module_handle);
        self.next_module_handle += 1;

        link_state.module_handles.push(handle);
        self.side_modules.insert(handle, loaded_module);
        self.side_module_names
            .insert(module_path.as_ref().to_owned(), handle);

        let init = {
            let instance = instance.clone();
            move |store: &mut StoreMut| {
                // No idea at which point this should be called. Also, apparently, there isn't an actual
                // implementation of the init function that does anything (that I can find?), so it doesn't
                // matter anyway.
                init(&instance, store).map_err(LinkError::InitializationError)?;


                Ok(())
            }
        };
        let reloc = {
            let instance = instance.clone();
            move |store: &mut StoreMut| {
                // No idea at which point this should be called. Also, apparently, there isn't an actual
                // implementation of the init function that does anything (that I can find?), so it doesn't
                // matter anyway.

                call_initialization_function(&instance, store, "__wasm_apply_data_relocs")?;

                Result::<(),LinkError>::Ok(())
            }
        };
        let ctor = {
            let instance = instance.clone();
            move |store: &mut StoreMut| {
                // No idea at which point this should be called. Also, apparently, there isn't an actual
                // implementation of the init function that does anything (that I can find?), so it doesn't
                // matter anyway.

                call_initialization_function(&instance, store, "__wasm_call_ctors")?;

                Result::<(),LinkError>::Ok(())
            }
        };

        link_state.reloc_callbacks.push(Box::new(reloc));
        link_state.ctor_callbacks.push(Box::new(ctor));
        link_state.init_callbacks.push(Box::new(init));

        Ok(handle)
    }

    fn resolve_export(
        &self,
        store: &mut impl AsStoreMut,
        module_handle: Option<ModuleHandle>,
        symbol: &str,
    ) -> Result<ResolvedExport, ResolveError> {
        match module_handle {
            Some(module_handle) => {
                let (instance, memory_base) = if module_handle == MAIN_MODULE_HANDLE {
                    (
                        self.main_instance()
                            .expect("Internal error: main_instance not set"),
                        0,
                    )
                } else {
                    let module = self
                        .side_modules
                        .get(&module_handle)
                        .ok_or(ResolveError::InvalidModuleHandle)?;
                    (&module.instance, module.memory_base)
                };

                self.resolve_export_from(store, symbol, instance, memory_base)
            }

            None => {
                // TODO: this would be the place to support RTLD_NEXT
                if let Some(instance) = self.main_instance() {
                    match self.resolve_export_from(store, symbol, instance, 0) {
                        Ok(export) => return Ok(export),
                        Err(ResolveError::MissingExport) => (),
                        Err(e) => return Err(e),
                    }
                }

                for module in self.side_modules.values() {
                    match self.resolve_export_from(
                        store,
                        symbol,
                        &module.instance,
                        module.memory_base,
                    ) {
                        Ok(export) => return Ok(export),
                        Err(ResolveError::MissingExport) => (),
                        Err(e) => return Err(e),
                    }
                }

                Err(ResolveError::MissingExport)
            }
        }
    }

    fn resolve_export_from(
        &self,
        store: &mut impl AsStoreMut,
        symbol: &str,
        instance: &Instance,
        memory_base: u64,
    ) -> Result<ResolvedExport, ResolveError> {
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

    pub fn append_to_function_table(
        &self,
        store: &mut impl AsStoreMut,
        func: Function,
    ) -> Result<u32, LinkError> {
        let table = &self.indirect_function_table;

        table
            .grow(store, 1, func.into())
            .map_err(LinkError::TableAllocationError)
    }

    fn finalize_pending_globals(
        &self,
        store: &mut impl AsStoreMut,
        unresolved_globals: &Vec<UnresolvedGlobal>,
    ) -> Result<(), LinkError> {
        for unresolved in unresolved_globals {
            match unresolved {
                UnresolvedGlobal::Mem(name, global) => {
                    let resolved = self.resolve_export(store, None, name).map_err(|e| {
                        LinkError::UnresolvedGlobal("GOT.mem".to_string(), name.clone(), e)
                    })?;
                    if let ResolvedExport::Global(addr) = resolved {
                        set_integer_global(store, name, global, addr)?;
                    } else {
                        return Err(LinkError::UnresolvedGlobal(
                            "GOT.mem".to_string(),
                            name.clone(),
                            ResolveError::MissingExport,
                        ));
                    }
                }
                UnresolvedGlobal::Func(name, global) => {
                    let resolved = self.resolve_export(store, None, name).map_err(|e| {
                        LinkError::UnresolvedGlobal("GOT.func".to_string(), name.clone(), e)
                    })?;
                    if let ResolvedExport::Function(func) = resolved {
                        let func_handle = self.append_to_function_table(store, func)?;
                        set_integer_global(store, name, global, func_handle as u64)?;
                    } else {
                        return Err(LinkError::UnresolvedGlobal(
                            "GOT.func".to_string(),
                            name.clone(),
                            ResolveError::MissingExport,
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

async fn locate_module(module_path: &Path, fs: &WasiFs) -> Result<(PathBuf, Vec<u8>), LinkError> {
    async fn try_load(
        fs: &WasiFsRoot,
        path: impl AsRef<Path>,
    ) -> Result<(PathBuf, Vec<u8>), FsError> {
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
        Ok((path.as_ref().to_owned(), buf))
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
        // Note: a path without a slash does *not* look at the current directory. This is by design.

        // TODO: implement RUNPATH once it's supported by clang and wasmparser
        // TODO: support $ORIGIN and ${ORIGIN} in RUNPATH

        for path in DEFAULT_RUNTIME_PATH {
            if let Ok(ret) = try_load(&fs.root_fs, Path::new(path).join(module_path)).await {
                return Ok(ret);
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

    let reader = wasmparser::Dylink0SectionReader::new(wasmparser::BinaryReader::new(&section, 0));

    let mut mem_info = None;
    let mut needed = None;

    let mut import_info = Vec::new();
    let mut export_info = Vec::new();

    for subsection in reader {
        let subsection = subsection?;
        match subsection {
            wasmparser::Dylink0Subsection::MemInfo(m) => {
                mem_info = Some(m);
            }

            wasmparser::Dylink0Subsection::Needed(n) => {
                needed = Some(n.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            }

            // Import info is used for declaring weak symbols
            wasmparser::Dylink0Subsection::ImportInfo(i) => {
                import_info.extend(i.iter().map(|i|ImportInfo::from(i)));
            }
            // Export info is used for declaring visibility-hidden symbols
            wasmparser::Dylink0Subsection::ExportInfo(e) => {
                export_info.extend(e.iter().map(|e|ExportInfo::from(e)));
            }
            wasmparser::Dylink0Subsection::Unknown { .. } => (),
        }
    }

    Ok(DylinkInfo {
        mem_info: mem_info.unwrap_or(wasmparser::MemInfo {
            memory_size: 0,
            memory_alignment: 0,
            table_size: 0,
            table_alignment: 0,
        }),
        needed: needed.unwrap_or_default(),
        import_info: import_info,
        export_info: export_info,
    })
}

fn define_integer_global_import(
    store: &mut impl AsStoreMut,
    import: &ImportType,
    value: u64,
) -> Result<Global, LinkError> {
    let ExternType::Global(GlobalType { ty, mutability }) = import.ty() else {
        return Err(LinkError::BadImport(
            import.module().to_string(),
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
                import.module().to_string(),
                import.name().to_string(),
                import.ty().clone(),
            ));
        }
    };

    Ok(global)
}

fn set_integer_global(
    store: &mut impl AsStoreMut,
    name: &str,
    global: &Global,
    value: u64,
) -> Result<(), LinkError> {
    match global.ty(store).ty {
        Type::I32 => global
            .set(store, Value::I32(value as i32))
            .map_err(|e| LinkError::GlobalUpdateFailed(name.to_owned(), e))?,
        Type::I64 => global
            .set(store, Value::I64(value as i64))
            .map_err(|e| LinkError::GlobalUpdateFailed(name.to_owned(), e))?,
        _ => {
            // This should be caught by resolve_global_import, so just panic here
            unreachable!("Internal error: expected global of type I32 or I64");
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
