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
//!
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
//! runtime when they're first called. This *does*, however, mean that link errors can happen
//! at runtime, after the linker has reported successful linking of the modules. Such errors
//! are turned into a [`WasiError::DlSymbolResolutionFailed`] error and will terminate
//! execution completely.
//!
//! # Threading Support
//!
//! The linker supports the concept of "Instance Groups", which are multiple instances
//! of the same module tree. This corresponds very closely to WASIX threads, but is
//! named an instance group so as to keep the logic decoupled from the threading logic
//! in WASIX.
//!
//! Each instance group has its own store, indirect function table, and stack pointer,
//! but shares its memory with every other instance group. Note that even though the
//! underlying memory is the same, we need to create a new [`Memory`] instance
//! for each group via [`Memory::share_in_store`]. Also, when placing a symbol
//! in the function table, the linker always updates all function tables at the same
//! time. This is because function "pointers" can be passed across instance groups
//! (read: sent to other threads) by the guest code, so all function tables should
//! have exactly the same content at all times.
//!
//! One important aspect of instance groups is that they do *not* share the same store;
//! this lets us put different instance groups on different OS threads. However, this
//! also means that one call to [`Linker::load_module`], etc. cannot update every
//! instance group as each one has its own function table. To make the linker work
//! across threads, we need a "stop-the-world" lock on every instance group. The group
//! the load/resolve request originates from sets a flag, which other instance
//! groups are required to check periodically by calling [`Linker::do_pending_link_operations`].
//! Once all instance groups are stopped in that function, the original can proceed to
//! perform the operation, and report its results to all other instance groups so they
//! can make the same changes to their function table as well.
//!
//! In WASIX, the periodic check is performed at the start of most (but not all) syscalls.
//! This means a thread that doesn't make any syscalls can potentially block all other
//! threads if a DL operation is performed. This also means that two instance groups
//! cannot co-exist on the same OS thread, as the first one will block the OS thread
//! and the second can't enter the "lock" again to let the first continue its work.
//!
//! To also get cooperation from threads that are waiting in a syscall, a
//! [`Signal::Sigwakeup`](wasmer_wasix_types::wasi::Signal::Sigwakeup) signal is sent to
//! all threads when a DL operation needs to be synchronized.
//!
//! # About TLS
//!
//! Each instance of each group gets its own TLS area, so there are 4 cases to consider:
//!     * Main instance of main module: TLS area will be allocated by the compiler, and be
//!       placed at the start of the memory region requested by the `dylink.0` section.
//!     * Main instance of side modules: Almost same as main module, but tls_base will be
//!       non-zero because side modules get a non-zero memory_base. It is very important
//!       to note that the main instance of a side module lives in the instance group
//!       that initially loads it in. This **does not** have to be the main instance
//!       group.
//!     * Other instances of main module: Each worker thread gets its TLS area
//!       allocated by the code in pthread_create, and a pointer to the TLS area is passed
//!       through the thread start args. This pointer is read by the code in thread_spawn,
//!       and passed through to us as part of the environment's memory layout.
//!     * Other instances of side modules: This is where the linker comes in. When the
//!       new instance is created, the linker will call its `__wasix_init_tls` function,
//!       which is responsible for setting up the TLS area for the thread.
//!
//! Since we only want to call `__wasix_init_tls` for non-main instances of side modules,
//! it is enough to call it only within [`InstanceGroupState::instantiate_side_module_from_linker`].
//!
//! # Module Loading
//!
//! Module loading happens as an orchestrated effort between the shared linker state, the
//! state of the instance group that started (or "instigated") the operation, and other
//! instance groups. Access to a set of instances is required for resolution of exports,
//! which is why the linker state alone (which only stores modules) is not enough.
//!
//! Even though most (if not all) operations require access to both the shared linker state
//! and a/the instance group state, they're separated into three sets:
//!     * Operations that deal with metadata exist as impls on [`LinkerState`]. These take
//!       a (read-only) instance group state for export resolution, as well as a
//!       [`StoreRef`](wasmer::StoreRef). They're guaranteed not to alter the store or the
//!       instance group state.
//!     * Operations that deal with the actual instances (instantiating, putting symbols in the
//!       function table, etc.) and are started by the instigating group exist as impls on
//!       [`InstanceGroupState`] that also take a mutable reference to the shared linker state, and
//!       require it to be locked for writing. These operations can and will update the linker state,
//!       mainly to store symbol resolution records.
//!     * Operations that deal with replicating changes to instances from another thread also exits
//!       as impls on [`InstanceGroupState`], but take a read-only reference to the shared linker
//!       state. This is important because all the information needed for replicating the change to
//!       the instigating group's instances should already be in the linker state. See
//!       [`InstanceGroupState::populate_imports_from_linker`] and
//!       [`InstanceGroupState::instantiate_side_module_from_linker`] for the two most important ones.
//!
//! Module loading generally works by going through these steps:
//!     * [`LinkerState::load_module_tree`] loads modules (and their needed modules) and assigns
//!       module handles
//!     * Then, for each new module:
//!         * Memory and table space is allocated
//!         * Imports are resolved (see next section)
//!         * The module is instantiated
//!     * After all modules have been instantiated, pending imports (resulting from circular
//!       dependencies) are resolved
//!     * Finally, module initializers are called
//!
//! ## Symbol resolution
//!
//! To support replicating operations from the instigating group to other groups, symbol resolution
//! happens in 3 steps:
//!     * [`LinkerState::resolve_symbols`] goes through the imports of a soon-to-be-loaded module,
//!       recording the imports as [`NeededSymbolResolutionKey`]s and creating
//!       [`InProgressSymbolResolution`]s in response to each one.
//!     * [`InstanceGroupState::populate_imports_from_link_state`] then goes through the results
//!       and resolves each import to its final value, while also recording enough information (in the
//!       shape of [`SymbolResolutionResult`]s) for other groups to resolve the symbol from their own
//!       instances.
//!     * Finally, instances are created and finalized, and initializers are called.
//!
//! ## Stub functions
//!
//! As noted above, stub functions are generated in response to circular dependencies. The stub
//! functions do take previous symbol resolution records into account, so that the stub corresponding
//! to a single import cannot resolve to different exports in different groups. If no such record is
//! found, then a new record is created by the stub function. However, there's a catch.
//!
//! It must be noted that, during initialization, the shared linker state has to remain write-locked
//! so as to prevent other threads from starting another operation (the replication logic only works
//! with one active operation at a time). Stub functions need a write lock on the shared linker state
//! to store new resolution records, and as such, they can't store resolution records if they're
//! called in response to a module's initialization routines. This can happen easily if:
//! * A side module is needed by the main
//! * That side module accesses any libc functions, such as printing something to stdout.
//!
//! To work around this, stub functions only *try* to lock the shared linker state, and if they can't,
//! they won't store anything. A follow-up call to the stub function can resolve the symbol again,
//! store it for use by further calls to the function, and also create a resolution record. This does
//! create a few hard-to-reach edge cases:
//!     * If the symbol happens to resolve differently between the two calls to the stub, unpredictable
//!       behavior can happen; however, this is impossible in the current implementation.
//!     * If the shared state is locked by a different instance group, then the stub won't store its
//!       lookup results anyway, even though it could have if it had waited.
//!
//! ## Locating side modules
//!
//! Side modules are located according to these steps:
//!     * If the name contains a slash (/), it is treated as a relative or absolute path.   
//!     * Otherwise, the name is searched for in `/lib`, `/usr/lib` and `/usr/local/lib`.
//!       LD_LIBRARY_PATH is not supported yet.
//!
//! # Building dynamically-linked modules
//!
//! Note that building modules that conform the specific requirements of this linker requires
//! careful configuration of clang. A PIC sysroot is required. The steps to build a main
//! module are:
//!
//! ```bash
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
//! ```bash
//! clang-19 \
//!   --target=wasm32-wasi --sysroot=/path/to/sysroot32-pic \
//!   -matomics -mbulk-memory -mmutable-globals -pthread \
//!   -mthread-model posix -ftls-model=local-exec \
//!   -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
//!   -D_WASI_EMULATED_PROCESS_CLOCKS \
//!   # We need PIC
//!   -fPIC \
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
    collections::{BTreeMap, HashMap},
    ffi::OsStr,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier, Mutex, MutexGuard, RwLock, RwLockWriteGuard, TryLockError,
    },
};

use bus::Bus;
use derive_more::Debug;
use shared_buffer::OwnedBuffer;
use tracing::trace;
use virtual_fs::{AsyncReadExt, FileSystem, FsError};
use virtual_mio::InlineWaker;
use wasmer::{
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, ExternType, Function, FunctionEnv,
    FunctionEnvMut, FunctionType, Global, GlobalType, ImportType, Imports, Instance,
    InstantiationError, Memory, MemoryError, Module, RuntimeError, StoreMut, Table, Tag, Type,
    Value, WasmTypeList, WASM_PAGE_SIZE,
};
use wasmer_wasix_types::wasix::WasiMemoryLayout;

use crate::{
    fs::WasiFsRoot, import_object_for_all_wasi_versions, runtime::module_cache::HashedModuleData,
    Runtime, SpawnError, WasiEnv, WasiError, WasiFs, WasiFunctionEnv, WasiModuleTreeHandles,
    WasiProcess, WasiThreadId,
};

use super::{WasiModuleInstanceHandles, WasiState};

// Module handle 0 is always the main module. Side modules get handles starting from 1.
pub static MAIN_MODULE_HANDLE: ModuleHandle = ModuleHandle(0);
static INVALID_MODULE_HANDLE: ModuleHandle = ModuleHandle(u32::MAX);

static MAIN_MODULE_MEMORY_BASE: u64 = 0;
// Need to keep the zeroth index null to catch null function pointers at runtime
static MAIN_MODULE_TABLE_BASE: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

struct AllocatedPage {
    // The base_ptr is mutable, and will move forward as memory is allocated from the page.
    base_ptr: u32,

    // The amount of memory remaining until the end of the allocated region. Despite the
    // name of this struct, the region does not have to be only one page.
    remaining: u32,
}

// Used to allocate and manage memory for dynamic modules that are loaded in or
// out, since each module may request a specific amount of memory to be allocated
// for it before starting it up.
// TODO: Only supports Memory32, should implement proper Memory64 support
struct MemoryAllocator {
    allocated_pages: Vec<AllocatedPage>,
}

impl MemoryAllocator {
    pub fn new() -> Self {
        Self {
            allocated_pages: vec![],
        }
    }

    pub fn allocate(
        &mut self,
        memory: &Memory,
        store: &mut impl AsStoreMut,
        size: u32,
        alignment: u32,
    ) -> Result<u32, MemoryError> {
        match self.allocate_in_existing_pages(size, alignment) {
            Some(base_ptr) => Ok(base_ptr),
            None => self.allocate_new_page(memory, store, size),
        }
    }

    // Finds a page which has enough free memory for the request, and allocates in it.
    // Returns the address of the allocated region if one was found.
    fn allocate_in_existing_pages(&mut self, size: u32, alignment: u32) -> Option<u32> {
        // A type to hold intermediate search results. The idea is to allocate on the page
        // that has the least amount of free space, so we can later satisfy larger allocation
        // requests without having to allocate entire new pages.
        struct CandidatePage {
            index: usize,
            base_ptr: u32,
            to_add: u32,
            remaining_free: u32,
        }

        impl PartialEq for CandidatePage {
            fn eq(&self, other: &Self) -> bool {
                self.remaining_free == other.remaining_free
            }
        }

        impl Eq for CandidatePage {}

        impl PartialOrd for CandidatePage {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for CandidatePage {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.remaining_free.cmp(&other.remaining_free)
            }
        }

        let mut candidates = std::collections::BinaryHeap::new();

        for (index, page) in self.allocated_pages.iter().enumerate() {
            // Offset for proper alignment
            let offset = if page.base_ptr % alignment == 0 {
                0
            } else {
                alignment - (page.base_ptr % alignment)
            };

            if page.remaining >= offset + size {
                candidates.push(std::cmp::Reverse(CandidatePage {
                    index,
                    base_ptr: page.base_ptr + offset,
                    to_add: offset + size,
                    remaining_free: page.remaining - offset - size,
                }));
            }
        }

        candidates.pop().map(|elected| {
            let page = &mut self.allocated_pages[elected.0.index];

            trace!(
                free = page.remaining,
                base_ptr = elected.0.base_ptr,
                "Found existing memory page with sufficient space"
            );

            page.base_ptr += elected.0.to_add;
            page.remaining -= elected.0.to_add;
            elected.0.base_ptr
        })
    }

    fn allocate_new_page(
        &mut self,
        memory: &Memory,
        store: &mut impl AsStoreMut,
        size: u32,
    ) -> Result<u32, MemoryError> {
        // No need to account for alignment here, as pages are already 64k-aligned
        let to_grow = size.div_ceil(WASM_PAGE_SIZE as u32);
        let pages = memory.grow(store, to_grow)?;

        let base_ptr = pages.0 * WASM_PAGE_SIZE as u32;
        let total_allocated = to_grow * WASM_PAGE_SIZE as u32;

        // The initial size bytes are already allocated, rest goes into the list
        if total_allocated > size {
            self.allocated_pages.push(AllocatedPage {
                base_ptr: base_ptr + size,
                remaining: total_allocated - size,
            });
        }

        trace!(
            page_count = to_grow,
            size,
            base_ptr,
            "Allocated new memory page(s) to accommodate requested memory"
        );

        Ok(base_ptr)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LinkError {
    #[error("Cannot access linker through a dead instance group")]
    InstanceGroupIsDead,

    #[error("Main module is missing a required import: {0}")]
    MissingMainModuleImport(String),

    #[error("Failed to spawn module: {0}")]
    SpawnError(#[from] SpawnError),

    #[error("Failed to instantiate module: {0}")]
    InstantiationError(#[from] InstantiationError),

    #[error("Memory allocation error: {0}")]
    MemoryAllocationError(#[from] MemoryError),

    #[error("Failed to allocate function table indices: {0}")]
    TableAllocationError(RuntimeError),

    #[error("Failed to find shared library {0}: {1}")]
    SharedLibraryMissing(String, LocateModuleError),

    #[error("Module is not a dynamic library")]
    NotDynamicLibrary,

    #[error("Failed to parse dylink.0 section: {0}")]
    Dylink0SectionParseError(#[from] wasmparser::BinaryReaderError),

    #[error("Unresolved global '{0}'.{1} due to: {2}")]
    UnresolvedGlobal(String, String, Box<ResolveError>),

    #[error("Failed to update global {0} due to: {1}")]
    GlobalUpdateFailed(String, RuntimeError),

    #[error("Expected global to be of type I32 or I64: '{0}'.{1}")]
    NonIntegerGlobal(String, String),

    #[error("Bad known import: '{0}'.{1} of type {2:?}")]
    BadImport(String, String, ExternType),

    #[error(
        "Import could not be satisfied because of type mismatch: '{0}'.{1}, expected {2:?}, found {3:?}"
    )]
    ImportTypeMismatch(String, String, ExternType, ExternType),

    #[error("Expected import to be a function: '{0}'.{1}")]
    ImportMustBeFunction(&'static str, String),

    #[error("Expected export {0} to be a function, found: {1:?}")]
    ExportMustBeFunction(String, ExternType),

    #[error("Failed to initialize instance: {0}")]
    InitializationError(anyhow::Error),

    #[error("Initialization function has invalid signature: {0}")]
    InitFuncWithInvalidSignature(String),

    #[error("Initialization function {0} failed to run: {1}")]
    InitFunctionFailed(String, RuntimeError),

    #[error("Failed to initialize WASI(X) module handles: {0}")]
    MainModuleHandleInitFailed(ExportError),

    #[error("Module does not export a TLS initialization routine")]
    MissingTlsInitializer,
}

#[derive(Debug)]
pub enum LocateModuleError {
    Single(FsError),
    Multiple(Vec<(PathBuf, FsError)>),
}

impl std::fmt::Display for LocateModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocateModuleError::Single(e) => std::fmt::Display::fmt(&e, f),
            LocateModuleError::Multiple(errors) => {
                for (path, error) in errors {
                    write!(f, "\n    {}: {}", path.display(), error)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
enum PartiallyResolvedExport {
    Function(Function),
    Global(u64),
    Tls {
        // The offset relative to the TLS area of the instance. Kept so we
        // can re-resolve for other instance groups.
        offset: u64,
        // The final address of the symbol for the current instance group.
        final_addr: u64,
    },
}

pub enum ResolvedExport {
    Function { func_ptr: u64 },

    // Contains the offset of the global in memory, with memory_base/tls_base accounted for
    // See: https://github.com/WebAssembly/tool-conventions/blob/main/DynamicLinking.md#exports
    Global { data_ptr: u64 },
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

    #[error("Failed to allocate function table indices: {0}")]
    TableAllocationError(RuntimeError),

    #[error("Cannot access linker through a dead instance group")]
    InstanceGroupIsDead,

    #[error("Failed to perform pending DL operation: {0}")]
    PendingDlOperationFailed(#[from] LinkError),
}

#[derive(Debug, Clone)]
pub struct DylinkInfo {
    pub mem_info: wasmparser::MemInfo,
    pub needed: Vec<String>,
    pub import_metadata: HashMap<(String, String), wasmparser::SymbolFlags>,
    pub export_metadata: HashMap<String, wasmparser::SymbolFlags>,
}

pub struct LinkedMainModule {
    pub instance: Instance,
    pub memory: Memory,
    pub indirect_function_table: Table,
    pub stack_low: u64,
    pub stack_high: u64,
}

#[derive(Debug)]
enum UnresolvedGlobal {
    // A GOT.mem entry, should be resolved to an exported global from another module.
    Mem(NeededSymbolResolutionKey, Global),
    // A GOT.func entry, should be resolved to the address of an exported function
    // from another module (e.g. an index into __indirect_function_table).
    Func(NeededSymbolResolutionKey, Global),
}

impl UnresolvedGlobal {
    fn key(&self) -> &NeededSymbolResolutionKey {
        match self {
            Self::Func(key, _) => key,
            Self::Mem(key, _) => key,
        }
    }

    fn global(&self) -> &Global {
        match self {
            Self::Func(_, global) => global,
            Self::Mem(_, global) => global,
        }
    }

    fn import_module(&self) -> &str {
        match self {
            Self::Func(..) => "GOT.func",
            Self::Mem(..) => "GOT.mem",
        }
    }
}

#[derive(Debug)]
struct PendingFunctionResolutionFromLinkerState {
    resolved_from: ModuleHandle,
    name: String,
    function_table_index: u32,
}

#[derive(Debug)]
struct PendingTlsPointer {
    global: Global,
    resolved_from: ModuleHandle,
    offset: u64,
}

// Used only when processing a module load operation from another instance group.
#[derive(Debug, Default)]
struct PendingResolutionsFromLinker {
    functions: Vec<PendingFunctionResolutionFromLinkerState>,
    tls: Vec<PendingTlsPointer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NeededSymbolResolutionKey {
    module_handle: ModuleHandle,
    // Corresponds to the first identifier, such as env in env.memory. Both "module"
    // names come from the WASM spec, unfortunately, so we can't change them.
    // We only resolve from a well-known set of modules, namely "env", "GOT.mem" and
    // "GOT.func", so this doesn't need to be an owned string.
    import_module: String,
    import_name: String,
}

#[derive(Debug)]
enum InProgressSymbolResolution {
    Function(ModuleHandle),
    StubFunction(FunctionType),
    // May or may not be a TLS symbol.
    MemGlobal(ModuleHandle),
    FuncGlobal(ModuleHandle),
    UnresolvedMemGlobal,
    UnresolvedFuncGlobal,
}

#[derive(Debug)]
struct InProgressModuleLoad {
    handle: ModuleHandle,
    module: Module,
    dylink_info: DylinkInfo,
}

#[derive(Default, Debug)]
struct InProgressLinkState {
    // All modules loaded in by this link operation, in the order they were loaded in.
    new_modules: Vec<InProgressModuleLoad>,

    // Modules that are currently being loaded in from the FS due to needed sections.
    pending_module_paths: Vec<PathBuf>,

    // Collection of intermediate symbol resolution results. This includes functions
    // that have been found but not appended to the function tables yet, as well as
    // unresolved globals.
    symbols: HashMap<NeededSymbolResolutionKey, InProgressSymbolResolution>,

    unresolved_globals: Vec<UnresolvedGlobal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SymbolResolutionKey {
    Needed(NeededSymbolResolutionKey),
    Requested(String),
}

#[derive(Debug)]
pub enum SymbolResolutionResult {
    // The symbol was resolved to a global address. We don't resolve again because
    // the value of globals and the memory_base for each module and all of its instances
    // is fixed, and we can't nuke globals in the same way we do with functions. The end
    // goal is to have new instance groups behave exactly the same as existing instance
    // groups; since existing instance groups will have a (possibly invalid) pointer
    // into memory from when this global still existed, we do the same for new instance
    // groups.
    // The case of unresolved globals is not mentioned here, since it can't exist once
    // a link operation is complete.
    Memory(u64),
    // The symbol was resolved to a global address, but the global is a TLS variable.
    // Each instance of each module has a different TLS area, and TLS symbols must be
    // resolved again every time.
    Tls {
        resolved_from: ModuleHandle,
        offset: u64,
    },
    // The symbol was resolved to a function export with the same name from this module.
    // it is expected that the symbol resolves to an export of the correct type.
    Function {
        ty: FunctionType,
        resolved_from: ModuleHandle,
    },
    // Same deal as above, but a pointer was generated and placed in the function table.
    FunctionPointer {
        resolved_from: ModuleHandle,
        function_table_index: u32,
    },
    // The symbol failed to resolve, but it's a function so we can create a stub. The
    // first call to any stub associated with this symbol must update the resolution
    // record to point to the module the function was resolved from.
    StubFunction(FunctionType),
}

// Used to communicate the result of an operation that happened in one
// instance group to all others
#[derive(Debug, Clone)]
enum DlOperation {
    LoadModules(Vec<ModuleHandle>),
    ResolveFunction {
        name: String,
        resolved_from: ModuleHandle,
        // This should match the current length of each instance group's function table
        // minus one. Otherwise, we're out of sync and an error has been encountered.
        function_table_index: u32,
    },
}

struct DlModule {
    module: Module,
    dylink_info: DylinkInfo,
    memory_base: u64,
    table_base: u64,
}

struct DlInstance {
    instance: Instance,
    #[allow(dead_code)]
    instance_handles: WasiModuleInstanceHandles,
    tls_base: u64,
}

struct InstanceGroupState {
    main_instance: Option<Instance>,
    main_instance_tls_base: u64,

    side_instances: HashMap<ModuleHandle, DlInstance>,

    stack_pointer: Global,
    memory: Memory,
    indirect_function_table: Table,

    // Once the dl_operation_pending flag is set, a barrier is created and broadcast
    // by the instigating group, which others must use to rendezvous with it.
    recv_pending_operation_barrier: bus::BusReader<Arc<Barrier>>,
    // The corresponding sender is stored in the shared linker state, and is used
    // by the instigating instance group  to broadcast the results.
    recv_pending_operation: bus::BusReader<DlOperation>,
}

struct LinkerState {
    main_module: Module,
    main_module_dylink_info: DylinkInfo,

    // We used to have an issue where spawning instances out-of-order in new threads
    // would break globals. That has since been fixed. However, spawning in the same
    // order helps with diagnosing potential linker issues, so we're keeping the
    // hack from back then.
    // To ensure the same order, we use a BTreeMap here, which means when we
    // iterate over it, we'll get the modules from lowest handle to highest, and
    // order is preserved.
    side_modules: BTreeMap<ModuleHandle, DlModule>,
    side_modules_by_name: HashMap<PathBuf, ModuleHandle>,
    next_module_handle: u32,

    memory_allocator: MemoryAllocator,
    heap_base: u64,

    symbol_resolution_records: HashMap<SymbolResolutionKey, SymbolResolutionResult>,

    send_pending_operation_barrier: bus::Bus<Arc<Barrier>>,
    send_pending_operation: bus::Bus<DlOperation>,
}

/// The linker is responsible for loading and linking dynamic modules at runtime,
/// and managing the shared memory and indirect function table.
/// Each linker instance represents a specific instance group. Cloning a linker
/// instance does *not* create a new instance group though; the clone will refer
/// to the same group as the original.
#[derive(Clone)]
pub struct Linker {
    linker_state: Arc<RwLock<LinkerState>>,
    instance_group_state: Arc<Mutex<Option<InstanceGroupState>>>,

    // Is a DL operation pending? This is the cheapest way I know of to let each
    // instance group check if an operation is *not* pending, which is the case
    // 99.99% of the time. Uses Relaxed ordering all the time, since we don't
    // even particularly care about a missed read of this value. A later call can
    // always pick the flag up and start waiting for the DL operation to complete.
    // This should only be written after the linker state has been exclusively
    // locked for writing.
    dl_operation_pending: Arc<AtomicBool>,
}

// This macro exists to ensure we don't get into a deadlock with another pending
// DL operation. the linker state must be locked for write *ONLY THROUGH THIS
// MACRO*. Bad things happen otherwise.
// We also need a lock on the specific group's state here, because if there is a
// pending DL operation we need to apply, that'll require mutable access to the
// group's state. Rather than just lock it within the macro and cause a potential
// deadlock, the macro requires to lock be acquired beforehand and passed in.
macro_rules! write_linker_state {
    ($guard:ident, $linker:expr, $group_state:ident, $ctx:ident) => {
        #[allow(unused_mut)]
        let mut $guard = loop {
            match $linker.linker_state.try_write() {
                Ok(guard) => break guard,
                Err(TryLockError::WouldBlock) => {
                    // The group that holds the lock is most likely waiting for an op
                    // to finish, so we should help it with that...
                    let env = $ctx.as_ref();
                    let mut store = $ctx.as_store_mut();
                    $linker.do_pending_link_operations_internal($group_state, &mut store, &env)?;
                    // ... and sleep for a while before attempting the lock again, so
                    // everything has time to settle. We don't care too much about the
                    // performance of the actual DL ops, since those will be few and
                    // far in between (hopefully!).
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(TryLockError::Poisoned(_)) => panic!("The linker state's lock is poisoned"),
            }
        };
    };
}

macro_rules! lock_instance_group_state {
    ($guard:ident, $state:ident, $linker:expr, $err:expr) => {
        let mut $guard = $linker.instance_group_state.lock().unwrap();
        if $guard.is_none() {
            return Err($err);
        }
        let $state = $guard.deref_mut().as_mut().unwrap();
    };
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

        trace!(?dylink_section, "Loading main module");

        let mut imports = import_object_for_all_wasi_versions(main_module, store, &func_env.env);

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

        // TODO: do we need to add one to the table length requested by the module? I _think_
        // clang takes the null funcref at index zero into account in the table size, so we
        // _may_ not need this. Need to experiment and figure this out.
        let expected_table_length =
            dylink_section.mem_info.table_size + MAIN_MODULE_TABLE_BASE as u32;
        // Make sure the function table is as big as the dylink.0 section expects it to be
        if indirect_function_table.size(store) < expected_table_length {
            indirect_function_table
                .grow(
                    store,
                    expected_table_length - indirect_function_table.size(store),
                    Value::FuncRef(None),
                )
                .map_err(LinkError::TableAllocationError)?;
        }

        trace!(
            size = indirect_function_table.size(store),
            "Indirect function table initial size"
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
        // because it's always placed directly after the main module's data
        memory.grow_at_least(store, stack_high)?;

        trace!(
            memory_pages = ?memory.grow(store, 0).unwrap(),
            stack_low,
            stack_high,
            "Memory layout"
        );

        let stack_pointer_import = main_module
            .imports()
            .find(|i| i.module() == "env" && i.name() == "__stack_pointer")
            .ok_or(LinkError::MissingMainModuleImport(
                "__stack_pointer".to_string(),
            ))?;

        let stack_pointer = define_integer_global_import(store, &stack_pointer_import, stack_high)?;

        let mut barrier_tx = Bus::new(1);
        let barrier_rx = barrier_tx.add_rx();
        let mut operation_tx = Bus::new(1);
        let operation_rx = operation_tx.add_rx();

        let mut instance_group = InstanceGroupState {
            main_instance: None,
            // Every main instance's TLS area is at the start of its memory,
            // which is 0 for the main module's main instance
            main_instance_tls_base: MAIN_MODULE_MEMORY_BASE,
            side_instances: HashMap::new(),
            stack_pointer,
            memory: memory.clone(),
            indirect_function_table: indirect_function_table.clone(),
            recv_pending_operation_barrier: barrier_rx,
            recv_pending_operation: operation_rx,
        };

        let mut linker_state = LinkerState {
            main_module: main_module.clone(),
            main_module_dylink_info: dylink_section,
            side_modules: BTreeMap::new(),
            side_modules_by_name: HashMap::new(),
            next_module_handle: 1,
            memory_allocator: MemoryAllocator::new(),
            heap_base: stack_high,
            symbol_resolution_records: HashMap::new(),
            send_pending_operation_barrier: barrier_tx,
            send_pending_operation: operation_tx,
        };

        let mut link_state = InProgressLinkState::default();

        let well_known_imports = [
            ("env", "__memory_base", MAIN_MODULE_MEMORY_BASE),
            ("env", "__table_base", MAIN_MODULE_TABLE_BASE),
            ("GOT.mem", "__stack_high", stack_high),
            ("GOT.mem", "__stack_low", stack_low),
            ("GOT.mem", "__heap_base", stack_high),
        ];

        trace!("Resolving main module's symbols");
        linker_state.resolve_symbols(
            &instance_group,
            store,
            main_module,
            MAIN_MODULE_HANDLE,
            &mut link_state,
            &well_known_imports,
        )?;

        trace!("Populating main module's imports object");
        instance_group.populate_imports_from_link_state(
            MAIN_MODULE_HANDLE,
            &mut linker_state,
            &mut link_state,
            store,
            main_module,
            &mut imports,
            &func_env.env,
            &well_known_imports,
        )?;

        // TODO: figure out which way is faster (stubs in main or stubs in sides),
        // use that ordering. My *guess* is that, since main exports all the libc
        // functions and those are called frequently by basically any code, then giving
        // stubs to main will be faster, but we need numbers before we decide this.
        let main_instance = Instance::new(store, main_module, &imports)?;
        instance_group.main_instance = Some(main_instance.clone());

        for needed in linker_state.main_module_dylink_info.needed.clone() {
            // A successful load_module will add the module to the side_modules list,
            // from which symbols can be resolved in the following call to
            // guard.resolve_imports.
            trace!(name = needed, "Loading module needed by main");
            let wasi_env = func_env.data(store);
            linker_state.load_module_tree(
                needed,
                &mut link_state,
                &wasi_env.runtime,
                &wasi_env.state,
                Option::<&[&str]>::None,
            )?;
        }

        for module_handle in link_state
            .new_modules
            .iter()
            .map(|m| m.handle)
            .collect::<Vec<_>>()
        {
            trace!(?module_handle, "Instantiating module");
            instance_group.instantiate_side_module_from_link_state(
                &mut linker_state,
                store,
                &func_env.env,
                &mut link_state,
                module_handle,
            )?;
        }

        let linker = Self {
            linker_state: Arc::new(RwLock::new(linker_state)),
            instance_group_state: Arc::new(Mutex::new(Some(instance_group))),
            dl_operation_pending: Arc::new(AtomicBool::new(false)),
        };

        let stack_layout = WasiMemoryLayout {
            stack_lower: stack_low,
            stack_upper: stack_high,
            stack_size: stack_high - stack_low,
            guard_size: 0,
            tls_base: Some(MAIN_MODULE_MEMORY_BASE),
        };
        let module_handles = WasiModuleTreeHandles::Dynamic {
            linker: linker.clone(),
            main_module_instance_handles: WasiModuleInstanceHandles::new(
                memory.clone(),
                store,
                main_instance.clone(),
            ),
        };

        func_env
            .initialize_handles_and_layout(
                store,
                main_instance.clone(),
                module_handles,
                Some(stack_layout),
                true,
            )
            .map_err(LinkError::MainModuleHandleInitFailed)?;

        // This function is exported from PIE executables, and needs to be run before calling
        // _initialize or _start. More info:
        // https://github.com/WebAssembly/tool-conventions/blob/main/DynamicLinking.md
        trace!("Calling data relocator function for main module");
        call_initialization_function::<()>(&main_instance, store, "__wasm_apply_data_relocs")?;

        {
            let group_guard = linker.instance_group_state.lock().unwrap();
            let mut linker_state = linker.linker_state.write().unwrap();
            trace!("Finalizing linking of main module");
            linker.finalize_link_operation(group_guard, &mut linker_state, store, link_state)?;
        }

        trace!("Calling main module's _initialize function");
        call_initialization_function::<()>(&main_instance, store, "_initialize")?;

        trace!("Link complete");

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

    pub fn create_instance_group(
        &self,
        parent_ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        store: &mut StoreMut<'_>,
        func_env: &mut WasiFunctionEnv,
    ) -> Result<(Self, LinkedMainModule), LinkError> {
        trace!("Spawning new instance group");

        lock_instance_group_state!(
            parent_group_state_guard,
            parent_group_state,
            self,
            LinkError::InstanceGroupIsDead
        );

        // Can't have other groups do operations that don't get replicated to
        // the new group, so lock the linker state while we work.
        write_linker_state!(linker_state, self, parent_group_state, parent_ctx);

        let parent_store = parent_ctx.as_store_mut();

        let main_module = linker_state.main_module.clone();
        let memory = parent_group_state
            .memory
            .share_in_store(&parent_store, store)?;

        let mut imports = import_object_for_all_wasi_versions(&main_module, store, &func_env.env);

        let indirect_function_table = Table::new(
            store,
            parent_group_state.indirect_function_table.ty(&parent_store),
            Value::FuncRef(None),
        )
        .map_err(LinkError::TableAllocationError)?;

        let expected_table_length = parent_group_state
            .indirect_function_table
            .size(&parent_store);
        // Grow the table to be as big as the parent's
        if indirect_function_table.size(store) < expected_table_length {
            indirect_function_table
                .grow(
                    store,
                    expected_table_length - indirect_function_table.size(store),
                    Value::FuncRef(None),
                )
                .map_err(LinkError::TableAllocationError)?;
        }

        trace!(
            size = indirect_function_table.size(store),
            "Indirect function table initial size"
        );

        // Since threads initialize their own stack space, we can only rely on the layout being
        // initialized beforehand, which is the case with the thread_spawn syscall.
        // FIXME: this needs to become a parameter if we ever decouple the linker from WASIX
        let (stack_low, stack_high, tls_base) = {
            let layout = &func_env.env.as_ref(store).layout;
            (
                layout.stack_lower,
                layout.stack_upper,
                layout.tls_base.expect(
                    "tls_base must be set in memory layout of new instance group's main instance",
                ),
            )
        };

        trace!(stack_low, stack_high, "Memory layout");

        let stack_pointer_import = main_module
            .imports()
            .find(|i| i.module() == "env" && i.name() == "__stack_pointer")
            .ok_or(LinkError::MissingMainModuleImport(
                "__stack_pointer".to_string(),
            ))?;

        // WASIX threads initialize their own stack pointer global in wasi_thread_start,
        // so no need to initialize it to a value here.
        let stack_pointer = define_integer_global_import(store, &stack_pointer_import, 0)?;

        let barrier_rx = linker_state.send_pending_operation_barrier.add_rx();
        let operation_rx = linker_state.send_pending_operation.add_rx();

        let mut instance_group = InstanceGroupState {
            main_instance: None,
            main_instance_tls_base: tls_base,
            side_instances: HashMap::new(),
            stack_pointer,
            memory: memory.clone(),
            indirect_function_table: indirect_function_table.clone(),
            recv_pending_operation_barrier: barrier_rx,
            recv_pending_operation: operation_rx,
        };

        let mut pending_resolutions = PendingResolutionsFromLinker::default();

        let well_known_imports = [
            ("env", "__memory_base", MAIN_MODULE_MEMORY_BASE),
            ("env", "__table_base", MAIN_MODULE_TABLE_BASE),
            ("GOT.mem", "__stack_high", stack_high),
            ("GOT.mem", "__stack_low", stack_low),
            ("GOT.mem", "__heap_base", linker_state.heap_base),
        ];

        trace!("Populating imports object for new instance group's main instance");
        instance_group.populate_imports_from_linker(
            MAIN_MODULE_HANDLE,
            &linker_state,
            store,
            &main_module,
            &mut imports,
            &func_env.env,
            &well_known_imports,
            &mut pending_resolutions,
        )?;

        let main_instance = Instance::new(store, &main_module, &imports)?;

        instance_group.main_instance = Some(main_instance.clone());

        for side in &linker_state.side_modules {
            trace!(module_handle = ?side.0, "Instantiating existing side module");
            instance_group.instantiate_side_module_from_linker(
                &linker_state,
                store,
                &func_env.env,
                *side.0,
                &mut pending_resolutions,
            )?;
        }

        trace!("Finalizing pending functions");
        instance_group.finalize_pending_resolutions_from_linker(&pending_resolutions, store)?;

        trace!("Applying externally-requested function table entries");
        instance_group.apply_requested_symbols_from_linker(store, &linker_state)?;

        let linker = Self {
            linker_state: self.linker_state.clone(),
            instance_group_state: Arc::new(Mutex::new(Some(instance_group))),
            dl_operation_pending: self.dl_operation_pending.clone(),
        };

        let module_handles = WasiModuleTreeHandles::Dynamic {
            linker: linker.clone(),
            main_module_instance_handles: WasiModuleInstanceHandles::new(
                memory.clone(),
                store,
                main_instance.clone(),
            ),
        };

        func_env
            .initialize_handles_and_layout(
                store,
                main_instance.clone(),
                module_handles,
                None,
                false,
            )
            .map_err(LinkError::MainModuleHandleInitFailed)?;

        trace!("Instance group spawned successfully");

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

    pub fn shutdown_instance_group(
        &self,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<(), LinkError> {
        trace!("Shutting instance group down");

        let mut guard = self.instance_group_state.lock().unwrap();
        match guard.as_mut() {
            None => Ok(()),
            Some(group_state) => {
                // We need to do this even if the results of an incoming dl op will be thrown away;
                // this is because the instigating group will have counted us and we need to hit the
                // barrier twice to unblock everybody else.
                write_linker_state!(linker_state, self, group_state, ctx);
                guard.take();
                drop(linker_state);

                trace!("Instance group shut down");

                Ok(())
            }
        }
    }

    /// Loads a side module from the given path, linking it against the existing module tree
    /// and instantiating it. Symbols from the module can then be retrieved by calling
    /// [`Linker::resolve_export`].
    pub fn load_module(
        &self,
        module_path: impl AsRef<Path>,
        library_path: &[impl AsRef<Path>],
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<ModuleHandle, LinkError> {
        let module_path = module_path.as_ref();

        trace!(?module_path, "Loading module");

        lock_instance_group_state!(
            group_state_guard,
            group_state,
            self,
            LinkError::InstanceGroupIsDead
        );

        // TODO: differentiate between an actual link error and an error that occurs as the
        // result of a pending operation that needs to be applied first. Currently, errors
        // from pending ops are treated as link errors and just reported to guest code rather
        // than terminating the process.
        write_linker_state!(linker_state, self, group_state, ctx);

        let mut link_state = InProgressLinkState::default();
        let env = ctx.as_ref();
        let mut store = ctx.as_store_mut();

        trace!("Loading module tree for requested module");
        let wasi_env = env.as_ref(&store);
        let module_handle = linker_state.load_module_tree(
            module_path,
            &mut link_state,
            &wasi_env.runtime,
            &wasi_env.state,
            Some(library_path),
        )?;

        let new_modules = link_state
            .new_modules
            .iter()
            .map(|m| m.handle)
            .collect::<Vec<_>>();

        for handle in &new_modules {
            trace!(?module_handle, "Instantiating module");
            group_state.instantiate_side_module_from_link_state(
                &mut linker_state,
                &mut store,
                &env,
                &mut link_state,
                *handle,
            )?;
        }

        trace!("Finalizing link");
        self.finalize_link_operation(group_state_guard, &mut linker_state, &mut store, link_state)?;

        if !new_modules.is_empty() {
            // The group state is unlocked for stub functions, now lock it again
            lock_instance_group_state!(
                group_state_guard,
                group_state,
                self,
                LinkError::InstanceGroupIsDead
            );

            self.synchronize_link_operation(
                DlOperation::LoadModules(new_modules),
                linker_state,
                group_state,
                &ctx.data().process,
                ctx.data().tid(),
            );
        }

        // FIXME: If we fail at an intermediate step, we should reset the linker's state, a la:
        // if result.is_err() {
        //     let mut guard = self.state.lock().unwrap();
        //     let memory = guard.memory.clone();

        //     for module_handle in link_state.module_handles.iter().cloned() {
        //         let module = guard.side_modules.remove(&module_handle).unwrap();
        //         guard
        //             .side_module_names
        //             .retain(|_, handle| *handle != module_handle);
        //         // We already have an error we need to report, so ignore memory deallocation errors
        //         _ = guard
        //             .memory_allocator
        //             .deallocate(&memory, store, module.memory_base);
        //     }
        // }

        trace!("Module load complete");

        Ok(module_handle)
    }

    fn finalize_link_operation(
        &self,
        // Take ownership of the guard and drop it ourselves to ensure no deadlock can happen
        mut group_state_guard: MutexGuard<'_, Option<InstanceGroupState>>,
        linker_state: &mut LinkerState,
        store: &mut impl AsStoreMut,
        link_state: InProgressLinkState,
    ) -> Result<(), LinkError> {
        let group_state = group_state_guard.as_mut().unwrap();

        trace!(?link_state, "Finalizing link operation");

        group_state.finalize_pending_globals(
            linker_state,
            store,
            &link_state.unresolved_globals,
        )?;

        let new_instances = link_state
            .new_modules
            .iter()
            .map(|m| group_state.side_instances[&m.handle].instance.clone())
            .collect::<Vec<_>>();

        // The instance group must be unlocked for the next step, since modules may need to resolve
        // stub functions and that requires a lock on the instance group's state
        drop(group_state_guard);

        trace!("Calling data relocation functions");
        for instance in &new_instances {
            call_initialization_function::<()>(instance, store, "__wasm_apply_data_relocs")?;
        }

        trace!("Calling ctor functions");
        for instance in &new_instances {
            call_initialization_function::<()>(instance, store, "__wasm_call_ctors")?;
        }

        Ok(())
    }

    // TODO: Support RTLD_NEXT
    /// Resolves an export from the module corresponding to the given module handle.
    /// Only functions and globals can be resolved.
    ///
    /// If the symbol is a global, the returned value will be the absolute address of
    /// the data corresponding to that global within the shared linear memory.
    ///
    /// If it's a function, it'll be placed into the indirect function table,
    /// which creates a "function pointer" that can be used from WASM code.
    pub fn resolve_export(
        &self,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        module_handle: Option<ModuleHandle>,
        symbol: &str,
    ) -> Result<ResolvedExport, ResolveError> {
        trace!(?module_handle, symbol, "Resolving symbol");

        let resolution_key = SymbolResolutionKey::Requested(symbol.to_string());

        lock_instance_group_state!(guard, group_state, self, ResolveError::InstanceGroupIsDead);

        if let Ok(linker_state) = self.linker_state.try_read() {
            if let Some(resolution) = linker_state.symbol_resolution_records.get(&resolution_key) {
                trace!(?resolution, "Already have a resolution for this symbol");
                match resolution {
                    SymbolResolutionResult::FunctionPointer {
                        function_table_index: addr,
                        ..
                    } => {
                        return Ok(ResolvedExport::Function {
                            func_ptr: *addr as u64,
                        })
                    }
                    SymbolResolutionResult::Memory(addr) => {
                        return Ok(ResolvedExport::Global { data_ptr: *addr })
                    }
                    SymbolResolutionResult::Tls {
                        resolved_from,
                        offset,
                    } => {
                        let tls_base = group_state.tls_base(*resolved_from);
                        return Ok(ResolvedExport::Global {
                            data_ptr: tls_base + offset,
                        });
                    }
                    r => panic!(
                        "Internal error: unexpected symbol resolution \
                        {r:?} for requested symbol {symbol}"
                    ),
                }
            }
        }

        write_linker_state!(linker_state, self, group_state, ctx);

        let mut store = ctx.as_store_mut();

        trace!("Resolving export");
        let (export, resolved_from) =
            group_state.resolve_export(&linker_state, &mut store, module_handle, symbol, false)?;

        trace!(?export, ?resolved_from, "Resolved export");

        match export {
            PartiallyResolvedExport::Global(addr) => {
                linker_state
                    .symbol_resolution_records
                    .insert(resolution_key, SymbolResolutionResult::Memory(addr));

                Ok(ResolvedExport::Global { data_ptr: addr })
            }
            PartiallyResolvedExport::Tls { offset, final_addr } => {
                linker_state.symbol_resolution_records.insert(
                    resolution_key,
                    SymbolResolutionResult::Tls {
                        resolved_from,
                        offset,
                    },
                );

                Ok(ResolvedExport::Global {
                    data_ptr: final_addr,
                })
            }
            PartiallyResolvedExport::Function(func) => {
                let func_ptr = group_state
                    .append_to_function_table(&mut store, func.clone())
                    .map_err(ResolveError::TableAllocationError)?;
                trace!(
                    ?func_ptr,
                    table_size = group_state.indirect_function_table.size(&store),
                    "Placed resolved function into table"
                );
                linker_state.symbol_resolution_records.insert(
                    resolution_key,
                    SymbolResolutionResult::FunctionPointer {
                        resolved_from,
                        function_table_index: func_ptr,
                    },
                );

                self.synchronize_link_operation(
                    DlOperation::ResolveFunction {
                        name: symbol.to_string(),
                        resolved_from,
                        function_table_index: func_ptr,
                    },
                    linker_state,
                    group_state,
                    &ctx.data().process,
                    ctx.data().tid(),
                );

                Ok(ResolvedExport::Function {
                    func_ptr: func_ptr as u64,
                })
            }
        }
    }

    pub fn is_handle_valid(
        &self,
        handle: ModuleHandle,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<bool, LinkError> {
        // Remember, trying to get a read lock here can deadlock if a dl op is pending
        lock_instance_group_state!(guard, group_state, self, LinkError::InstanceGroupIsDead);
        write_linker_state!(linker_state, self, group_state, ctx);
        Ok(linker_state.side_modules.contains_key(&handle))
    }

    // Note: the caller needs to have applied the link operation beforehand to ensure
    // there are no (recoverable) errors. This function can only have unrecoverable
    // errors (i.e. panics).
    fn synchronize_link_operation(
        &self,
        op: DlOperation,
        mut linker_state_write_lock: RwLockWriteGuard<LinkerState>,
        group_state: &mut InstanceGroupState,
        wasi_process: &WasiProcess,
        self_thread_id: WasiThreadId,
    ) {
        trace!(?op, "Synchronizing link operation");

        let num_groups = linker_state_write_lock.send_pending_operation.rx_count();

        if num_groups <= 1 {
            trace!("No other living instance groups, nothing to do");
            return;
        }

        // Create and broadcast the barrier, so we have a rendezvous point
        let barrier = Arc::new(Barrier::new(num_groups));
        if linker_state_write_lock
            .send_pending_operation_barrier
            .try_broadcast(barrier.clone())
            .is_err()
        {
            // The bus is given a capacity of one to ensure we can't ever get here
            // more than once concurrently.
            panic!("Internal error: more than one synchronized link operation active")
        }

        // Set the flag, so others know they should stop now
        self.dl_operation_pending.store(true, Ordering::SeqCst);

        trace!("Signalling wasix threads to wake up");
        for thread in wasi_process
            .all_threads()
            .into_iter()
            .filter(|tid| *tid != self_thread_id)
        {
            // Signal all threads to wake them up if they're sleeping or idle
            wasi_process.signal_thread(&thread, wasmer_wasix_types::wasi::Signal::Sigwakeup);
        }

        trace!("Waiting at barrier");
        // Wait for all other threads to hit the barrier
        barrier.wait();

        trace!("All threads now processing dl op");

        // Reset the flag once everybody's seen it
        self.dl_operation_pending.store(false, Ordering::SeqCst);

        // Now we broadcast the actual operation. This has to happen before
        // we release the write lock, since exclusive access to the bus is
        // required.
        if linker_state_write_lock
            .send_pending_operation
            .try_broadcast(op.clone())
            .is_err()
        {
            // Same deal with the bus capacity
            panic!("Internal error: more than one synchronized link operation active")
        }

        // Now that everyone's at a safe point, we can unlock the shared state
        // and take another read lock. This is safe because everybody else will
        // also be taking only a read lock between the two barrier waits, and
        // no write locks can happen.
        trace!("Unlocking linker state");
        drop(linker_state_write_lock);
        let linker_state_read_lock = self.linker_state.read().unwrap();

        // Read and drop the barrier and operation from our own receivers, so
        // the bus is freed up
        _ = group_state.recv_pending_operation_barrier.recv().unwrap();
        _ = group_state.recv_pending_operation.recv().unwrap();

        // Second barrier, to make sure everyone applied the change. Necessary
        // because another thread may exit do_pending_link_operations and acquire
        // a write lock before anybody else has had the chance to get a read lock
        // without this wait in place.
        trace!("Waiting for other threads to finish processing the dl op");
        barrier.wait();

        // Drop the read lock after everyone is done.
        drop(linker_state_read_lock);

        trace!("Synchronization complete");
    }

    pub(crate) fn do_pending_link_operations(
        &self,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fast: bool,
    ) -> Result<(), LinkError> {
        // If no operation is pending, we can return immediately. This is the
        // hot path. If we happen to miss an operation that we would have
        // caught, no big deal; this will be called again later. However,
        // in the case where we raise a signal and it's caught by another thread,
        // we can't have this read go missing, otherwise the other thread will
        // sleep again and miss the notification. Hence the option to pick fast or
        // slow.
        if !self.dl_operation_pending.load(if fast {
            Ordering::Relaxed
        } else {
            Ordering::SeqCst
        }) {
            return Ok(());
        }

        lock_instance_group_state!(guard, group_state, self, LinkError::InstanceGroupIsDead);

        let env = ctx.as_ref();
        let mut store = ctx.as_store_mut();
        self.do_pending_link_operations_internal(group_state, &mut store, &env)
    }

    fn do_pending_link_operations_internal(
        &self,
        group_state: &mut InstanceGroupState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
    ) -> Result<(), LinkError> {
        if !self.dl_operation_pending.load(Ordering::SeqCst) {
            return Ok(());
        }

        trace!("Pending link operation discovered, will process");

        // Receive and wait for the barrier.
        let barrier = group_state.recv_pending_operation_barrier.recv().expect(
            "Failed to receive barrier while a DL operation was \
            in progress; this condition can't be recovered from",
        );
        barrier.wait();

        trace!("Past the barrier, now processing operation");

        // After everyone, including the instigating group has rendezvoused at
        // the first barrier, the operation should have been broadcast.
        let op = group_state.recv_pending_operation.recv().unwrap();
        // Once past the barrier, the instigating group will downgrade its
        // lock to a read lock, so we can also get a read lock here.
        let linker_state = self.linker_state.read().unwrap();

        let result = group_state.apply_dl_operation(linker_state.deref(), op, store, env);

        trace!("Operation applied, now waiting at second barrier");

        // Rendezvous one more time to make sure everybody's done, and nobody's
        // going to start another DL operation before that happens.
        barrier.wait();
        // Drop the read lock after the
        drop(linker_state);

        trace!("Pending link operation applied successfully");

        result
    }
}

impl LinkerState {
    fn allocate_memory(
        &mut self,
        store: &mut impl AsStoreMut,
        memory: &Memory,
        mem_info: &wasmparser::MemInfo,
    ) -> Result<u64, MemoryError> {
        trace!(?mem_info, "Allocating memory");

        let new_size = if mem_info.memory_size == 0 {
            0
        } else {
            self.memory_allocator.allocate(
                memory,
                store,
                mem_info.memory_size,
                2_u32.pow(mem_info.memory_alignment),
            )? as u64
        };

        trace!(new_size, "Final size");

        Ok(new_size)
    }

    fn memory_base(&self, module_handle: ModuleHandle) -> u64 {
        if module_handle == MAIN_MODULE_HANDLE {
            MAIN_MODULE_MEMORY_BASE
        } else {
            self.side_modules
                .get(&module_handle)
                .expect("Internal error: bad module handle")
                .memory_base
        }
    }

    fn dylink_info(&self, module_handle: ModuleHandle) -> &DylinkInfo {
        if module_handle == MAIN_MODULE_HANDLE {
            &self.main_module_dylink_info
        } else {
            &self
                .side_modules
                .get(&module_handle)
                .expect("Internal error: bad module handle")
                .dylink_info
        }
    }

    // Resolves all imports for the given module, and places the results into
    // the in progress link state's symbol collection.
    // A follow-up call to [`InstanceGroupState::populate_imports_from_link_state`]
    // is needed to create a usable imports object, which needs to happen once per
    // instance group.
    // Each instance group has a different store, so the group ID corresponding
    // to the given store must be provided to resolve globals from the correct
    // instances.
    fn resolve_symbols(
        &self,
        group: &InstanceGroupState,
        store: &mut impl AsStoreMut,
        module: &Module,
        module_handle: ModuleHandle,
        link_state: &mut InProgressLinkState,
        // Used only to "skip over" well known imports, so we don't actually need the
        // u64 values. However, we use the same type as populate_imports to let calling
        // code construct the data only once.
        well_known_imports: &[(&str, &str, u64)],
    ) -> Result<(), LinkError> {
        trace!(?module_handle, "Resolving symbols");
        for import in module.imports() {
            // Skip over well known imports, since they'll be provided externally
            if well_known_imports
                .iter()
                .any(|i| i.0 == import.module() && i.1 == import.name())
            {
                trace!(?import, "Skipping resolution of well-known symbol");
                continue;
            }

            // Skip over the memory, function table and stack pointer imports as well
            match import.name() {
                "memory" | "__indirect_function_table" | "__stack_pointer" | "__c_longjmp" => {
                    trace!(?import, "Skipping resolution of special symbol");
                    continue;
                }
                _ => (),
            }

            match import.module() {
                "env" => {
                    let resolution = self.resolve_env_symbol(group, &import, store)?;
                    trace!(?import, ?resolution, "Symbol resolved");
                    link_state.symbols.insert(
                        NeededSymbolResolutionKey {
                            module_handle,
                            import_module: "env".to_owned(),
                            import_name: import.name().to_string(),
                        },
                        resolution,
                    );
                }
                "GOT.mem" => {
                    let resolution = self.resolve_got_mem_symbol(group, &import, store)?;
                    trace!(?import, ?resolution, "Symbol resolved");
                    link_state.symbols.insert(
                        NeededSymbolResolutionKey {
                            module_handle,
                            import_module: "GOT.mem".to_owned(),
                            import_name: import.name().to_string(),
                        },
                        resolution,
                    );
                }
                "GOT.func" => {
                    let resolution = self.resolve_got_func_symbol(group, &import, store)?;
                    trace!(?import, ?resolution, "Symbol resolved");
                    link_state.symbols.insert(
                        NeededSymbolResolutionKey {
                            module_handle,
                            import_module: "GOT.func".to_owned(),
                            import_name: import.name().to_string(),
                        },
                        resolution,
                    );
                }
                _ => (),
            }
        }

        trace!(?module_handle, "All symbols resolved");

        Ok(())
    }

    // Imports from the env module are:
    //   * the memory and indirect function table
    //   * well-known addresses, such as __stack_pointer and __memory_base
    //   * functions that are imported directly
    // resolve_env_symbol only handles the imported functions.
    fn resolve_env_symbol(
        &self,
        group: &InstanceGroupState,
        import: &ImportType,
        store: &impl AsStoreRef,
    ) -> Result<InProgressSymbolResolution, LinkError> {
        let ExternType::Function(import_func_ty) = import.ty() else {
            return Err(LinkError::ImportMustBeFunction(
                "env",
                import.name().to_string(),
            ));
        };

        let export = group.resolve_exported_symbol(import.name());

        match export {
            Some((module_handle, export)) => {
                let Extern::Function(export_func) = export else {
                    return Err(LinkError::ImportTypeMismatch(
                        "env".to_string(),
                        import.name().to_string(),
                        ExternType::Function(import_func_ty.clone()),
                        export.ty(store).clone(),
                    ));
                };

                if export_func.ty(store) != *import_func_ty {
                    return Err(LinkError::ImportTypeMismatch(
                        "env".to_string(),
                        import.name().to_string(),
                        ExternType::Function(import_func_ty.clone()),
                        export.ty(store).clone(),
                    ));
                }

                Ok(InProgressSymbolResolution::Function(module_handle))
            }
            None => {
                // The function may be exported from a module we have yet to link in,
                // or otherwise not be used by the module at all. We provide a stub that,
                // when called, will try to resolve the symbol and call it. This lets
                // us resolve circular dependencies, as well as letting modules that don't
                // actually use their imports run successfully.
                Ok(InProgressSymbolResolution::StubFunction(
                    import_func_ty.clone(),
                ))
            }
        }
    }

    // "Global" imports (i.e. imports from GOT.mem and GOT.func) are integer globals.
    // GOT.mem imports should point to the address of another module's data.
    fn resolve_got_mem_symbol(
        &self,
        group: &InstanceGroupState,
        import: &ImportType,
        store: &impl AsStoreRef,
    ) -> Result<InProgressSymbolResolution, LinkError> {
        let global_type = get_integer_global_type_from_import(import)?;

        match group.resolve_exported_symbol(import.name()) {
            Some((module_handle, export)) => {
                let ExternType::Global(global_type) = export.ty(store) else {
                    return Err(LinkError::ImportTypeMismatch(
                        "GOT.mem".to_string(),
                        import.name().to_string(),
                        ExternType::Global(global_type),
                        export.ty(store).clone(),
                    ));
                };

                if !matches!(global_type.ty, Type::I32 | Type::I64) {
                    return Err(LinkError::ImportTypeMismatch(
                        "GOT.mem".to_string(),
                        import.name().to_string(),
                        ExternType::Global(global_type),
                        export.ty(store).clone(),
                    ));
                }

                Ok(InProgressSymbolResolution::MemGlobal(module_handle))
            }
            None => Ok(InProgressSymbolResolution::UnresolvedMemGlobal),
        }
    }

    // "Global" imports (i.e. imports from GOT.mem and GOT.func) are integer globals.
    // GOT.func imports are function pointers (i.e. indices into the indirect function
    // table).
    fn resolve_got_func_symbol(
        &self,
        group: &InstanceGroupState,
        import: &ImportType,
        store: &impl AsStoreRef,
    ) -> Result<InProgressSymbolResolution, LinkError> {
        // Ensure the global is the correct type (i32 or i64)
        let _ = get_integer_global_type_from_import(import)?;

        match group.resolve_exported_symbol(import.name()) {
            Some((module_handle, export)) => {
                let ExternType::Function(_) = export.ty(store) else {
                    return Err(LinkError::ExportMustBeFunction(
                        import.name().to_string(),
                        export.ty(store).clone(),
                    ));
                };

                Ok(InProgressSymbolResolution::FuncGlobal(module_handle))
            }
            None => Ok(InProgressSymbolResolution::UnresolvedFuncGlobal),
        }
    }

    // TODO: give loaded library a different wasi env that specifies its module handle
    // This function loads the module (and its needed modules) and puts the resulting `Module`s
    // in the linker state, while assigning handles and putting the handles in the in-progress
    // link state. The modules must then get their symbols resolved and be instantiated in the
    // order in which their handles exist in the link state.
    // Returns the handle of the originally requested module. This will be the last entry in
    // the link state's list of module handles, but only if the module was actually loaded; if
    // it was already loaded, the existing handle is returned.
    fn load_module_tree(
        &mut self,
        module_path: impl AsRef<Path>,
        link_state: &mut InProgressLinkState,
        runtime: &Arc<dyn Runtime + Send + Sync + 'static>,
        wasi_state: &WasiState,
        library_path: Option<&[impl AsRef<Path>]>,
    ) -> Result<ModuleHandle, LinkError> {
        let module_path = module_path.as_ref();
        trace!(?module_path, "Locating and loading module");

        if let Some(handle) = self.side_modules_by_name.get(module_path) {
            let handle = *handle;

            trace!(?module_path, ?handle, "Module was already loaded");

            return Ok(handle);
        }

        // Locate and load the module bytes
        let (full_path, module_bytes) =
            InlineWaker::block_on(locate_module(module_path, library_path, &wasi_state.fs))?;
        let module_data = HashedModuleData::new_sha256(module_bytes);

        trace!(?full_path, "Found module file");

        // TODO: this can be optimized by detecting early if the module is already
        // pending without loading its bytes
        if link_state.pending_module_paths.contains(&full_path) {
            trace!("Module is already pending, won't load again");
            // This is fine, since a non-empty pending_modules list means we are
            // recursively resolving needed modules. We don't use the handle
            // returned from this function for anything when running recursively
            // (see self.load_module call below).
            return Ok(INVALID_MODULE_HANDLE);
        }

        let module = runtime.load_module_sync(module_data)?;

        let dylink_info = parse_dylink0_section(&module)?;

        trace!(?dylink_info, "Loading side module");

        link_state.pending_module_paths.push(full_path);
        let num_pending_modules = link_state.pending_module_paths.len();
        let pop_pending_module = |link_state: &mut InProgressLinkState| {
            assert_eq!(
                num_pending_modules,
                link_state.pending_module_paths.len(),
                "Internal error: pending modules not maintained correctly"
            );
            link_state.pending_module_paths.pop().unwrap();
        };

        for needed in &dylink_info.needed {
            trace!(needed, "Loading needed side module");
            match self.load_module_tree(needed, link_state, runtime, wasi_state, library_path) {
                Ok(_) => (),
                Err(e) => {
                    pop_pending_module(link_state);
                    return Err(e);
                }
            }
        }

        let handle = ModuleHandle(self.next_module_handle);
        self.next_module_handle += 1;

        trace!(?module_path, ?handle, "Assigned handle to module");

        pop_pending_module(link_state);

        link_state.new_modules.push(InProgressModuleLoad {
            handle,
            dylink_info,
            module,
        });
        // Put the name in the linker state - the actual DlModule must be
        // constructed later by the instance group once table addresses are
        // allocated for the module.
        // TODO: allocate table here (at least logically)?
        self.side_modules_by_name
            .insert(module_path.to_owned(), handle);

        Ok(handle)
    }
}

impl InstanceGroupState {
    fn main_instance(&self) -> Option<&Instance> {
        self.main_instance.as_ref()
    }

    fn tls_base(&self, module_handle: ModuleHandle) -> u64 {
        if module_handle == MAIN_MODULE_HANDLE {
            // Main's TLS area is at the beginning of its memory
            self.main_instance_tls_base
        } else {
            self.side_instances
                .get(&module_handle)
                .expect("Internal error: bad module handle")
                .tls_base
        }
    }

    fn try_instance(&self, handle: ModuleHandle) -> Option<&Instance> {
        if handle == MAIN_MODULE_HANDLE {
            self.main_instance.as_ref()
        } else {
            self.side_instances.get(&handle).map(|i| &i.instance)
        }
    }

    fn instance(&self, handle: ModuleHandle) -> &Instance {
        self.try_instance(handle)
            .expect("Internal error: bad module handle or not instantiated in this group")
    }

    fn allocate_function_table(
        &mut self,
        store: &mut impl AsStoreMut,
        table_size: u32,
        table_alignment: u32,
    ) -> Result<u64, RuntimeError> {
        trace!(table_size, "Allocating table indices");

        let base_index = if table_size == 0 {
            0
        } else {
            let current_size = self.indirect_function_table.size(store);
            let alignment = 2_u32.pow(table_alignment);

            let offset = if current_size % alignment != 0 {
                alignment - (current_size % alignment)
            } else {
                0
            };

            let start = self.indirect_function_table.grow(
                store,
                table_size + offset,
                Value::FuncRef(None),
            )?;

            (start + offset) as u64
        };

        trace!(
            base_index,
            new_table_size = ?self.indirect_function_table.size(store),
            "Allocated table indices"
        );

        Ok(base_index)
    }

    fn append_to_function_table(
        &self,
        store: &mut impl AsStoreMut,
        func: Function,
    ) -> Result<u32, RuntimeError> {
        let table = &self.indirect_function_table;

        table.grow(store, 1, func.into())
    }

    fn append_to_function_table_at(
        &self,
        store: &mut impl AsStoreMut,
        func: Function,
        index: u32,
    ) -> Result<(), RuntimeError> {
        trace!(
            ?index,
            ?func,
            "Placing function into table at pre-defined index"
        );

        let table = &self.indirect_function_table;
        let size = table.size(store);

        if size <= index {
            table.grow(store, index - size + 1, Value::FuncRef(None))?;
            trace!(new_table_size = ?table.size(store), "Growing table");
        } else {
            let existing = table.get(store, index).unwrap();
            if let Value::FuncRef(Some(_)) = existing {
                panic!("Internal error: function table index {index} already occupied");
            }
        }

        table.set(store, index, Value::FuncRef(Some(func)))
    }

    fn instantiate_side_module_from_link_state(
        &mut self,
        linker_state: &mut LinkerState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        link_state: &mut InProgressLinkState,
        module_handle: ModuleHandle,
    ) -> Result<(), LinkError> {
        let Some(pending_module) = link_state
            .new_modules
            .iter()
            .find(|m| m.handle == module_handle)
        else {
            panic!(
                "Only recently-loaded modules in the link state can be instantiated \
                by instantiate_side_module_from_link_state"
            )
        };

        trace!(
            ?module_handle,
            ?link_state,
            "Instantiating module from link state"
        );

        let memory_base = linker_state.allocate_memory(
            store,
            &self.memory,
            &pending_module.dylink_info.mem_info,
        )?;
        let table_base = self
            .allocate_function_table(
                store,
                pending_module.dylink_info.mem_info.table_size,
                pending_module.dylink_info.mem_info.table_alignment,
            )
            .map_err(LinkError::TableAllocationError)?;

        trace!(
            memory_base,
            table_base,
            "Allocated memory and table for module"
        );

        let mut imports = import_object_for_all_wasi_versions(&pending_module.module, store, env);

        let well_known_imports = [
            ("env", "__memory_base", memory_base),
            ("env", "__table_base", table_base),
        ];

        let module = pending_module.module.clone();
        let dylink_info = pending_module.dylink_info.clone();

        trace!(?module_handle, "Resolving symbols");
        linker_state.resolve_symbols(
            self,
            store,
            &module,
            module_handle,
            link_state,
            &well_known_imports,
        )?;

        trace!(?module_handle, "Populating imports object");
        self.populate_imports_from_link_state(
            module_handle,
            linker_state,
            link_state,
            store,
            &module,
            &mut imports,
            env,
            &well_known_imports,
        )?;

        let instance = Instance::new(store, &module, &imports)?;

        let instance_handles =
            WasiModuleInstanceHandles::new(self.memory.clone(), store, instance.clone());

        let dl_module = DlModule {
            module,
            dylink_info,
            memory_base,
            table_base,
        };

        let dl_instance = DlInstance {
            instance: instance.clone(),
            instance_handles,
            // The TLS area of a side module's main instance is at the beginning
            // of its memory
            tls_base: memory_base,
        };

        linker_state.side_modules.insert(module_handle, dl_module);
        self.side_instances.insert(module_handle, dl_instance);

        trace!(?module_handle, "Module instantiated");

        Ok(())
    }

    // For when we receive a module loaded DL operation
    fn allocate_function_table_for_existing_module(
        &mut self,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        module_handle: ModuleHandle,
    ) -> Result<(), LinkError> {
        if self.side_instances.contains_key(&module_handle) {
            panic!(
                "Internal error: Module with handle {module_handle:?} \
                was already instantiated in this group"
            )
        };

        let dl_module = linker_state
            .side_modules
            .get(&module_handle)
            .expect("Internal error: module not loaded into linker");

        let table_base = self
            .allocate_function_table(
                store,
                dl_module.dylink_info.mem_info.table_size,
                dl_module.dylink_info.mem_info.table_alignment,
            )
            .map_err(LinkError::TableAllocationError)?;

        if table_base != dl_module.table_base {
            panic!("Internal error: table base out of sync with linker state");
        }

        trace!(table_base, "Allocated table indices for existing module");

        Ok(())
    }

    // For when we receive a module loaded DL operation
    fn instantiate_side_module_from_linker(
        &mut self,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
        module_handle: ModuleHandle,
        pending_resolutions: &mut PendingResolutionsFromLinker,
    ) -> Result<(), LinkError> {
        if self.side_instances.contains_key(&module_handle) {
            panic!(
                "Internal error: Module with handle {module_handle:?} \
                was already instantiated in this group"
            )
        };

        trace!(?module_handle, "Instantiating existing module from linker");

        let dl_module = linker_state
            .side_modules
            .get(&module_handle)
            .expect("Internal error: module not loaded into linker");

        let mut imports = import_object_for_all_wasi_versions(&dl_module.module, store, env);

        let well_known_imports = [
            ("env", "__memory_base", dl_module.memory_base),
            ("env", "__table_base", dl_module.table_base),
        ];

        trace!(?module_handle, "Populating imports object");
        self.populate_imports_from_linker(
            module_handle,
            linker_state,
            store,
            &dl_module.module,
            &mut imports,
            env,
            &well_known_imports,
            pending_resolutions,
        )?;

        let instance = Instance::new(store, &dl_module.module, &imports)?;

        // This is a non-main instance of a side module, so it needs a new TLS area
        let tls_base = call_initialization_function::<i32>(&instance, store, "__wasix_init_tls")?;

        let Some(tls_base) = tls_base else {
            return Err(LinkError::MissingTlsInitializer);
        };

        let instance_handles =
            WasiModuleInstanceHandles::new(self.memory.clone(), store, instance.clone());

        let dl_instance = DlInstance {
            instance: instance.clone(),
            instance_handles,
            tls_base: tls_base as u64,
        };

        self.side_instances.insert(module_handle, dl_instance);

        // Initialization logic must only be run once, so no init calls here; it is
        // assumed that the module was instantiated and its init callbacks were called
        // by whichever thread first called instantiate_side_module_from_link_state.

        trace!(?module_handle, "Existing module instantiated successfully");

        Ok(())
    }

    fn finalize_pending_resolutions_from_linker(
        &self,
        pending_resolutions: &PendingResolutionsFromLinker,
        store: &mut impl AsStoreMut,
    ) -> Result<(), LinkError> {
        trace!("Finalizing pending functions");

        for pending in &pending_resolutions.functions {
            let func = self
                .instance(pending.resolved_from)
                .exports
                .get_function(&pending.name)
                .unwrap_or_else(|e| {
                    panic!(
                        "Internal error: failed to resolve exported function {}: {e:?}",
                        pending.name
                    )
                });

            self.append_to_function_table_at(store, func.clone(), pending.function_table_index)
                .map_err(LinkError::TableAllocationError)?;

            trace!(?pending, "Placed pending function in table");
        }

        for tls in &pending_resolutions.tls {
            let tls_base = self.tls_base(tls.resolved_from);
            let final_addr = tls_base + tls.offset;
            set_integer_global(store, "<pending TLS global>", &tls.global, final_addr)?;
            trace!(?tls, tls_base, final_addr, "Setting pending TLS global");
        }

        Ok(())
    }

    fn apply_resolved_function(
        &self,
        store: &mut impl AsStoreMut,
        name: &str,
        resolved_from: ModuleHandle,
        function_table_index: u32,
    ) -> Result<(), LinkError> {
        trace!(
            ?name,
            ?resolved_from,
            function_table_index,
            "Applying resolved function"
        );

        let instance = &self
            .side_instances
            .get(&resolved_from)
            .unwrap_or_else(|| {
                panic!("Internal error: module {resolved_from:?} not loaded by this group")
            })
            .instance;

        let func = instance.exports.get_function(name).unwrap_or_else(|e| {
            panic!("Internal error: failed to resolve exported function {name}: {e:?}")
        });

        self.append_to_function_table_at(store, func.clone(), function_table_index)
            .map_err(LinkError::TableAllocationError)?;

        Ok(())
    }

    fn apply_dl_operation(
        &mut self,
        linker_state: &LinkerState,
        operation: DlOperation,
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<WasiEnv>,
    ) -> Result<(), LinkError> {
        trace!(?operation, "Applying operation");
        match operation {
            DlOperation::LoadModules(module_handles) => {
                // Allocate table first, since instantiating will put more stuff in the table
                // and we need to have the modules' own table space allocated before that. This
                // replicates the behavior of the instigating group.
                for handle in &module_handles {
                    self.allocate_function_table_for_existing_module(linker_state, store, *handle)?;
                }
                let mut pending_functions = PendingResolutionsFromLinker::default();
                for handle in module_handles {
                    self.instantiate_side_module_from_linker(
                        linker_state,
                        store,
                        env,
                        handle,
                        &mut pending_functions,
                    )?;
                }
                self.finalize_pending_resolutions_from_linker(&pending_functions, store)?;
            }
            DlOperation::ResolveFunction {
                name,
                resolved_from,
                function_table_index,
            } => self.apply_resolved_function(store, &name, resolved_from, function_table_index)?,
        };
        trace!("Operation applied successfully");
        Ok(())
    }

    fn apply_requested_symbols_from_linker(
        &self,
        store: &mut impl AsStoreMut,
        linker_state: &LinkerState,
    ) -> Result<(), LinkError> {
        for (key, val) in &linker_state.symbol_resolution_records {
            if let SymbolResolutionKey::Requested(name) = key {
                if let SymbolResolutionResult::FunctionPointer {
                    resolved_from,
                    function_table_index,
                } = val
                {
                    self.apply_resolved_function(
                        store,
                        name,
                        *resolved_from,
                        *function_table_index,
                    )?;
                }
            }
        }
        Ok(())
    }

    // TODO: take expected type into account in case multiple modules export the same name,
    // but with different types
    fn resolve_exported_symbol(&self, symbol: &str) -> Option<(ModuleHandle, &Extern)> {
        if let Some(export) = self
            .main_instance()
            .and_then(|instance| instance.exports.get_extern(symbol))
        {
            trace!(symbol, from = ?MAIN_MODULE_HANDLE, ?export, "Resolved exported symbol");
            Some((MAIN_MODULE_HANDLE, export))
        } else {
            for (handle, dl_instance) in &self.side_instances {
                if let Some(export) = dl_instance.instance.exports.get_extern(symbol) {
                    trace!(symbol, from = ?handle, ?export, "Resolved exported symbol");
                    return Some((*handle, export));
                }
            }

            trace!(symbol, "Failed to resolve exported symbol");
            None
        }
    }

    // This function populates the imports object for a single module from the given
    // in-progress link state.
    fn populate_imports_from_link_state(
        &self,
        module_handle: ModuleHandle,
        linker_state: &mut LinkerState,
        link_state: &mut InProgressLinkState,
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &mut Imports,
        env: &FunctionEnv<WasiEnv>,
        well_known_imports: &[(&str, &str, u64)],
    ) -> Result<(), LinkError> {
        trace!(?module_handle, "Populating imports object from link state");

        for import in module.imports() {
            // Skip non-DL-related import modules
            if !matches!(import.module(), "env" | "GOT.mem" | "GOT.func") {
                continue;
            }

            // Important env imports first!
            if import.module() == "env" {
                match import.name() {
                    "memory" => {
                        let ExternType::Memory(memory_ty) = import.ty() else {
                            return Err(LinkError::BadImport(
                                import.module().to_string(),
                                import.name().to_string(),
                                import.ty().clone(),
                            ));
                        };
                        trace!(?module_handle, ?import, "Main memory");

                        // Make sure the memory is big enough for the module being instantiated
                        let current_size = self.memory.grow(store, 0)?;
                        if current_size < memory_ty.minimum {
                            self.memory.grow(store, memory_ty.minimum - current_size)?;
                        }

                        imports.define(
                            import.module(),
                            import.name(),
                            Extern::Memory(self.memory.clone()),
                        );
                        continue;
                    }
                    "__indirect_function_table" => {
                        if !matches!(import.ty(), ExternType::Table(ty) if ty.ty == Type::FuncRef) {
                            return Err(LinkError::BadImport(
                                import.module().to_string(),
                                import.name().to_string(),
                                import.ty().clone(),
                            ));
                        }
                        trace!(?module_handle, ?import, "Function table");
                        imports.define(
                            import.module(),
                            import.name(),
                            Extern::Table(self.indirect_function_table.clone()),
                        );
                        continue;
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
                        trace!(?module_handle, ?import, "Stack pointer");
                        imports.define(
                            import.module(),
                            import.name(),
                            Extern::Global(self.stack_pointer.clone()),
                        );
                        continue;
                    }
                    // Clang generates this symbol when building modules that use EH-based sjlj.
                    "__c_longjmp" => {
                        if !matches!(import.ty(), ExternType::Tag(ty) if *ty.params == [Type::I32])
                        {
                            return Err(LinkError::BadImport(
                                import.module().to_string(),
                                import.name().to_string(),
                                import.ty().clone(),
                            ));
                        }
                        trace!(?module_handle, ?import, "setjmp/longjmp exception tag");
                        imports.define(
                            import.module(),
                            import.name(),
                            Tag::new(store, vec![Type::I32]),
                        );
                        continue;
                    }
                    _ => (),
                }
            }

            // Next, go over the well-known imports
            if let Some(well_known_value) = well_known_imports.iter().find_map(|i| {
                if i.0 == import.module() && i.1 == import.name() {
                    Some(i.2)
                } else {
                    None
                }
            }) {
                trace!(
                    ?module_handle,
                    ?import,
                    well_known_value,
                    "Well-known value"
                );
                imports.define(
                    import.module(),
                    import.name(),
                    define_integer_global_import(store, &import, well_known_value)?,
                );
                continue;
            }

            let key = NeededSymbolResolutionKey {
                module_handle,
                import_module: import.module().to_owned(),
                import_name: import.name().to_owned(),
            };

            // Finally, go through the resolution results
            let resolution = link_state.symbols.get(&key).unwrap_or_else(|| {
                panic!(
                    "Internal error: missing import resolution '{0}'.{1}",
                    key.import_module, key.import_name
                )
            });

            trace!(?module_handle, ?import, ?resolution, "Resolution");

            match resolution {
                InProgressSymbolResolution::Function(module_handle) => {
                    let func = self
                        .instance(*module_handle)
                        .exports
                        .get_function(import.name())
                        .expect("Internal error: bad in-progress symbol resolution");
                    imports.define(import.module(), import.name(), func.clone());
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Function {
                            ty: func.ty(store),
                            resolved_from: *module_handle,
                        },
                    );
                }

                InProgressSymbolResolution::StubFunction(func_ty) => {
                    let func = self.generate_stub_function(
                        store,
                        func_ty,
                        env,
                        module_handle,
                        import.name().to_string(),
                    );
                    imports.define(import.module(), import.name(), func);
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::StubFunction(func_ty.clone()),
                    );
                }

                InProgressSymbolResolution::MemGlobal(module_handle) => {
                    let export = self
                        .resolve_export_from(
                            store,
                            *module_handle,
                            import.name(),
                            self.instance(*module_handle),
                            linker_state.dylink_info(*module_handle),
                            linker_state.memory_base(*module_handle),
                            self.tls_base(*module_handle),
                            true,
                        )
                        .expect("Internal error: bad in-progress symbol resolution");

                    match export {
                        PartiallyResolvedExport::Global(addr) => {
                            trace!(?module_handle, ?import, addr, "Memory address");

                            let global =
                                define_integer_global_import(store, &import, addr).unwrap();

                            imports.define(import.module(), import.name(), global);
                            linker_state.symbol_resolution_records.insert(
                                SymbolResolutionKey::Needed(key.clone()),
                                SymbolResolutionResult::Memory(addr),
                            );
                        }

                        PartiallyResolvedExport::Tls { offset, final_addr } => {
                            trace!(?module_handle, ?import, offset, final_addr, "TLS address");

                            let global =
                                define_integer_global_import(store, &import, final_addr).unwrap();

                            imports.define(import.module(), import.name(), global);
                            linker_state.symbol_resolution_records.insert(
                                SymbolResolutionKey::Needed(key.clone()),
                                SymbolResolutionResult::Tls {
                                    resolved_from: *module_handle,
                                    offset,
                                },
                            );
                        }

                        PartiallyResolvedExport::Function(_) => {
                            panic!("Internal error: bad in-progress symbol resolution")
                        }
                    }
                }

                InProgressSymbolResolution::UnresolvedMemGlobal => {
                    let global = define_integer_global_import(store, &import, 0).unwrap();
                    imports.define(import.module(), import.name(), global.clone());

                    link_state
                        .unresolved_globals
                        .push(UnresolvedGlobal::Mem(key, global));
                }

                InProgressSymbolResolution::FuncGlobal(module_handle) => {
                    let func = self
                        .instance(*module_handle)
                        .exports
                        .get_function(import.name())
                        .expect("Internal error: bad in-progress symbol resolution");

                    let func_handle = self
                        .append_to_function_table(store, func.clone())
                        .map_err(LinkError::TableAllocationError)?;
                    trace!(
                        ?module_handle,
                        ?import,
                        index = func_handle,
                        "Allocated function table index"
                    );
                    let global =
                        define_integer_global_import(store, &import, func_handle as u64).unwrap();

                    imports.define(import.module(), import.name(), global);
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::FunctionPointer {
                            resolved_from: *module_handle,
                            function_table_index: func_handle,
                        },
                    );
                }

                InProgressSymbolResolution::UnresolvedFuncGlobal => {
                    let global = define_integer_global_import(store, &import, 0).unwrap();
                    imports.define(import.module(), import.name(), global.clone());

                    link_state
                        .unresolved_globals
                        .push(UnresolvedGlobal::Func(key, global));
                }
            }
        }

        trace!(?module_handle, "Imports object populated successfully");

        Ok(())
    }

    // For when we receive a module loaded DL operation
    fn populate_imports_from_linker(
        &self,
        module_handle: ModuleHandle,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &mut Imports,
        env: &FunctionEnv<WasiEnv>,
        well_known_imports: &[(&str, &str, u64)],
        pending_resolutions: &mut PendingResolutionsFromLinker,
    ) -> Result<(), LinkError> {
        trace!(
            ?module_handle,
            "Populating imports object for existing module from linker state"
        );

        for import in module.imports() {
            // Skip non-DL-related import modules
            if !matches!(import.module(), "env" | "GOT.mem" | "GOT.func") {
                continue;
            }

            // Important env imports first!
            if import.module() == "env" {
                match import.name() {
                    "memory" => {
                        if !matches!(import.ty(), ExternType::Memory(_)) {
                            return Err(LinkError::BadImport(
                                import.module().to_string(),
                                import.name().to_string(),
                                import.ty().clone(),
                            ));
                        }
                        trace!(?module_handle, ?import, "Main memory");
                        imports.define(
                            import.module(),
                            import.name(),
                            Extern::Memory(self.memory.clone()),
                        );
                        continue;
                    }
                    "__indirect_function_table" => {
                        if !matches!(import.ty(), ExternType::Table(ty) if ty.ty == Type::FuncRef) {
                            return Err(LinkError::BadImport(
                                import.module().to_string(),
                                import.name().to_string(),
                                import.ty().clone(),
                            ));
                        }
                        trace!(?module_handle, ?import, "Function table");
                        imports.define(
                            import.module(),
                            import.name(),
                            Extern::Table(self.indirect_function_table.clone()),
                        );
                        continue;
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
                        trace!(?module_handle, ?import, "Stack pointer");
                        imports.define(
                            import.module(),
                            import.name(),
                            Extern::Global(self.stack_pointer.clone()),
                        );
                        continue;
                    }
                    "__c_longjmp" => {
                        if !matches!(import.ty(), ExternType::Tag(ty) if *ty.params == [Type::I32])
                        {
                            return Err(LinkError::BadImport(
                                import.module().to_string(),
                                import.name().to_string(),
                                import.ty().clone(),
                            ));
                        }
                        trace!(?module_handle, ?import, "setjmp/longjmp exception tag");
                        imports.define(
                            import.module(),
                            import.name(),
                            Tag::new(store, vec![Type::I32]),
                        );
                        continue;
                    }
                    _ => (),
                }
            }

            // Next, go over the well-known imports
            if let Some(well_known_value) = well_known_imports.iter().find_map(|i| {
                if i.0 == import.module() && i.1 == import.name() {
                    Some(i.2)
                } else {
                    None
                }
            }) {
                trace!(
                    ?module_handle,
                    ?import,
                    well_known_value,
                    "Well-known value"
                );
                imports.define(
                    import.module(),
                    import.name(),
                    define_integer_global_import(store, &import, well_known_value)?,
                );
                continue;
            }

            let key = SymbolResolutionKey::Needed(NeededSymbolResolutionKey {
                module_handle,
                import_module: import.module().to_owned(),
                import_name: import.name().to_owned(),
            });

            // Finally, go through the resolution results
            let resolution = linker_state
                .symbol_resolution_records
                .get(&key)
                .unwrap_or_else(|| {
                    panic!(
                        "Internal error: missing symbol resolution record for '{0}'.{1}",
                        import.module(),
                        import.name()
                    )
                });

            trace!(?module_handle, ?import, ?resolution, "Resolution");

            match resolution {
                SymbolResolutionResult::Function { ty, resolved_from } => {
                    let func = match self.try_instance(*resolved_from) {
                        Some(instance) => {
                            trace!(
                                ?module_handle,
                                ?import,
                                ?resolved_from,
                                "Already have instance to resolve from"
                            );
                            instance
                                .exports
                                .get_function(import.name())
                                .expect("Internal error: failed to get exported function")
                                .clone()
                        }
                        // We may be loading a module tree, and the instance from which
                        // we're supposed to import the function may not exist yet, so
                        // we add in a stub, which will later use the resolution records
                        // to locate the function.
                        None => {
                            trace!(
                                ?module_handle,
                                ?import,
                                ?resolved_from,
                                "Don't have instance yet"
                            );

                            self.generate_stub_function(
                                store,
                                ty,
                                env,
                                module_handle,
                                import.name().to_owned(),
                            )
                        }
                    };
                    imports.define(import.module(), import.name(), func);
                }
                SymbolResolutionResult::StubFunction(ty) => {
                    let func = self.generate_stub_function(
                        store,
                        ty,
                        env,
                        module_handle,
                        import.name().to_owned(),
                    );
                    imports.define(import.module(), import.name(), func.clone());
                }
                SymbolResolutionResult::FunctionPointer {
                    resolved_from,
                    function_table_index,
                } => {
                    let func = self.try_instance(*resolved_from).map(|instance| {
                        instance
                            .exports
                            .get_function(import.name())
                            .unwrap_or_else(|e| {
                                panic!(
                                    "Internal error: failed to resolve function {}: {e:?}",
                                    import.name()
                                )
                            })
                    });
                    match func {
                        Some(func) => {
                            trace!(
                                ?module_handle,
                                ?import,
                                function_table_index,
                                "Placing function pointer into table"
                            );
                            self.append_to_function_table_at(
                                store,
                                func.clone(),
                                *function_table_index,
                            )
                            .map_err(LinkError::TableAllocationError)?;
                        }
                        None => {
                            trace!(
                                ?module_handle,
                                ?import,
                                function_table_index,
                                "Don't have instance yet, creating a pending function"
                            );
                            // Since we know the final value of the global, we can create it
                            // and just fill the function table in later
                            pending_resolutions.functions.push(
                                PendingFunctionResolutionFromLinkerState {
                                    resolved_from: *resolved_from,
                                    name: import.name().to_string(),
                                    function_table_index: *function_table_index,
                                },
                            );
                        }
                    };
                    let global =
                        define_integer_global_import(store, &import, *function_table_index as u64)?;
                    imports.define(import.module(), import.name(), global);
                }
                SymbolResolutionResult::Memory(addr) => {
                    let global = define_integer_global_import(store, &import, *addr)?;
                    imports.define(import.module(), import.name(), global);
                }
                SymbolResolutionResult::Tls {
                    resolved_from,
                    offset,
                } => {
                    let global = define_integer_global_import(store, &import, 0)?;
                    pending_resolutions.tls.push(PendingTlsPointer {
                        global: global.clone(),
                        resolved_from: *resolved_from,
                        offset: *offset,
                    });
                    imports.define(import.module(), import.name(), global);
                }
            }
        }

        Ok(())
    }

    // Resolve an export down to the "memory address" of the symbol. This is different from
    // `resolve_symbol`, which resolves a WASM export but does not care about its type and
    // does no further processing on the export itself.
    fn resolve_export(
        &self,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        module_handle: Option<ModuleHandle>,
        symbol: &str,
        allow_hidden: bool,
    ) -> Result<(PartiallyResolvedExport, ModuleHandle), ResolveError> {
        trace!(?module_handle, ?symbol, "Resolving export");
        match module_handle {
            Some(module_handle) => {
                let instance = self
                    .try_instance(module_handle)
                    .ok_or(ResolveError::InvalidModuleHandle)?;
                let tls_base = self.tls_base(module_handle);
                let memory_base = linker_state.memory_base(module_handle);
                let dylink_info = linker_state.dylink_info(module_handle);
                Ok((
                    self.resolve_export_from(
                        store,
                        module_handle,
                        symbol,
                        instance,
                        dylink_info,
                        memory_base,
                        tls_base,
                        allow_hidden,
                    )?,
                    module_handle,
                ))
            }

            None => {
                // TODO: this would be the place to support RTLD_NEXT
                if let Some(instance) = self.main_instance() {
                    match self.resolve_export_from(
                        store,
                        MAIN_MODULE_HANDLE,
                        symbol,
                        instance,
                        &linker_state.main_module_dylink_info,
                        linker_state.memory_base(MAIN_MODULE_HANDLE),
                        self.main_instance_tls_base,
                        allow_hidden,
                    ) {
                        Ok(export) => return Ok((export, MAIN_MODULE_HANDLE)),
                        Err(ResolveError::MissingExport) => (),
                        Err(e) => return Err(e),
                    }
                }

                for (handle, instance) in &self.side_instances {
                    match self.resolve_export_from(
                        store,
                        *handle,
                        symbol,
                        &instance.instance,
                        &linker_state.side_modules[handle].dylink_info,
                        linker_state.memory_base(*handle),
                        instance.tls_base,
                        allow_hidden,
                    ) {
                        Ok(export) => return Ok((export, *handle)),
                        Err(ResolveError::MissingExport) => (),
                        Err(e) => return Err(e),
                    }
                }

                trace!(
                    ?module_handle,
                    ?symbol,
                    "Failed to locate symbol after searching all instances"
                );
                Err(ResolveError::MissingExport)
            }
        }
    }

    fn resolve_export_from(
        &self,
        store: &mut impl AsStoreMut,
        module_handle: ModuleHandle,
        symbol: &str,
        instance: &Instance,
        dylink_info: &DylinkInfo,
        memory_base: u64,
        tls_base: u64,
        allow_hidden: bool,
    ) -> Result<PartiallyResolvedExport, ResolveError> {
        trace!(from = ?module_handle, symbol, "Resolving export from instance");
        let export = instance.exports.get_extern(symbol).ok_or_else(|| {
            trace!(from = ?module_handle, symbol, "Not found");
            ResolveError::MissingExport
        })?;

        if !allow_hidden
            && dylink_info
                .export_metadata
                .get(symbol)
                .map(|flags| flags.contains(wasmparser::SymbolFlags::VISIBILITY_HIDDEN))
                .unwrap_or(false)
        {
            return Err(ResolveError::MissingExport);
        }

        match export.ty(store) {
            ExternType::Function(_) => {
                trace!(from = ?module_handle, symbol, "Found function");
                Ok(PartiallyResolvedExport::Function(
                    Function::get_self_from_extern(export).unwrap().clone(),
                ))
            }
            ty @ ExternType::Global(_) => {
                let global = Global::get_self_from_extern(export).unwrap();
                let value = match global.get(store) {
                    Value::I32(value) => value as u64,
                    Value::I64(value) => value as u64,
                    _ => return Err(ResolveError::InvalidExportType(ty.clone())),
                };

                let is_tls = dylink_info
                    .export_metadata
                    .get(symbol)
                    .map(|flags| flags.contains(wasmparser::SymbolFlags::TLS))
                    .unwrap_or(false);

                if is_tls {
                    let final_value = value + tls_base;
                    trace!(
                        from = ?module_handle,
                        symbol,
                        value,
                        offset = value,
                        final_value,
                        "Found TLS global"
                    );
                    Ok(PartiallyResolvedExport::Tls {
                        offset: value,
                        final_addr: final_value,
                    })
                } else {
                    let final_value = value + memory_base;
                    trace!(from = ?module_handle, symbol, value, final_value, "Found global");
                    Ok(PartiallyResolvedExport::Global(final_value))
                }
            }
            ty => Err(ResolveError::InvalidExportType(ty.clone())),
        }
    }

    fn generate_stub_function(
        &self,
        store: &mut impl AsStoreMut,
        ty: &FunctionType,
        env: &FunctionEnv<WasiEnv>,
        requesting_module: ModuleHandle,
        name: String,
    ) -> Function {
        // TODO: only search through needed modules for the symbol. This requires the implementation
        // of needing/needed relationships between modules.
        trace!(?requesting_module, name, "Generating stub function");

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
                        trace!(?requesting_module, name, "Resolving stub function");

                        let (data, store) = env.data_and_store_mut();
                        let env_inner = data.inner();
                        // Safe to unwrap since we already know we're doing DL
                        let linker = env_inner.linker().unwrap();

                        // Try to lock the linker state. This *can* fail if a stub
                        // is called as part of the init logic for a module. If we
                        // can't lock the linker state, we just resolve the symbol
                        // but don't store the resolved function anywhere; a later
                        // call to the stub function can then resolve again. Since
                        // this module and the one that has the symbol have to be
                        // part of the same module tree, it's super-duper-unlikely
                        // that a second resolution of the symbol would return a
                        // different result and would indicate a problem with the
                        // implementation of the linker.
                        let linker_state = match linker.linker_state.try_write() {
                            Ok(guard) => {
                                trace!(
                                    ?requesting_module,
                                    name,
                                    "Locked linker state successfully"
                                );
                                Some(guard)
                            }
                            Err(TryLockError::WouldBlock) => {
                                trace!(?requesting_module, name, "Failed to lock linker state");
                                None
                            }
                            Err(TryLockError::Poisoned(_)) => {
                                *resolved_guard = Some(None);
                                return Err(mk_error());
                            }
                        };

                        let group_guard = linker.instance_group_state.lock().unwrap();
                        let Some(group_state) = group_guard.as_ref() else {
                            trace!(?requesting_module, name, "Instance group is already dead");
                            *resolved_guard = Some(None);
                            return Err(mk_error());
                        };

                        let resolution_key =
                            SymbolResolutionKey::Needed(NeededSymbolResolutionKey {
                                module_handle: requesting_module,
                                import_module: "env".to_owned(),
                                import_name: name.clone(),
                            });

                        match linker_state
                            .as_ref()
                            .and_then(|l| l.symbol_resolution_records.get(&resolution_key))
                        {
                            Some(SymbolResolutionResult::Function {
                                resolved_from,
                                ty: resolved_ty,
                            }) => {
                                trace!(
                                    ?requesting_module,
                                    name,
                                    "Function was already resolved in the linker"
                                );

                                if ty != *resolved_ty {
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                }

                                let func = group_state
                                    .instance(*resolved_from)
                                    .exports
                                    .get_function(&name)
                                    .unwrap()
                                    .clone();
                                *resolved_guard = Some(Some(func.clone()));
                                func
                            }
                            Some(SymbolResolutionResult::StubFunction(_)) | None => {
                                trace!(?requesting_module, name, "Resolving function");

                                let Some((resolved_from, export)) =
                                    group_state.resolve_exported_symbol(name.as_str())
                                else {
                                    trace!(?requesting_module, name, "Failed to resolve symbol");
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                };
                                let Extern::Function(func) = export else {
                                    trace!(
                                        ?requesting_module,
                                        name,
                                        ?resolved_from,
                                        "Resolved symbol is not a function"
                                    );
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                };
                                if func.ty(&store) != ty {
                                    trace!(
                                        ?requesting_module,
                                        name,
                                        ?resolved_from,
                                        "Resolved function has bad type"
                                    );
                                    *resolved_guard = Some(None);
                                    return Err(mk_error());
                                }

                                trace!(
                                    ?requesting_module,
                                    name,
                                    ?resolved_from,
                                    "Function resolved successfully"
                                );

                                // Only store the result if we can also put it in the linker's
                                // resolution records for other groups to find.
                                if let Some(mut linker_state) = linker_state {
                                    trace!(
                                        ?requesting_module,
                                        name,
                                        ?resolved_from,
                                        "Updating linker state with this resolution"
                                    );

                                    *resolved_guard = Some(Some(func.clone()));
                                    linker_state.symbol_resolution_records.insert(
                                        resolution_key,
                                        SymbolResolutionResult::Function {
                                            ty: func.ty(&store),
                                            resolved_from,
                                        },
                                    );
                                }

                                func.clone()
                            }
                            Some(resolution) => panic!(
                                "Internal error: resolution record for symbol \
                                {name} indicates non-function resolution {resolution:?}"
                            ),
                        }
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

    fn finalize_pending_globals(
        &self,
        linker_state: &mut LinkerState,
        store: &mut impl AsStoreMut,
        unresolved_globals: &Vec<UnresolvedGlobal>,
    ) -> Result<(), LinkError> {
        trace!("Finalizing pending globals");

        for unresolved in unresolved_globals {
            let key = unresolved.key();
            let import_metadata = &linker_state.dylink_info(key.module_handle).import_metadata;
            let is_weak = import_metadata
                .get(&(key.import_module.to_owned(), key.import_name.to_owned()))
                // clang seems to like putting the import-info in the "env" module
                // sometimes, so try that as well
                .or_else(|| import_metadata.get(&("env".to_owned(), key.import_name.to_owned())))
                .map(|flags| flags.contains(wasmparser::SymbolFlags::BINDING_WEAK))
                .unwrap_or(false);
            trace!(?unresolved, is_weak, "Resolving pending global");

            match (
                unresolved,
                self.resolve_export(linker_state, store, None, &key.import_name, true),
            ) {
                (
                    UnresolvedGlobal::Mem(key, global),
                    Ok((PartiallyResolvedExport::Global(addr), resolved_from)),
                ) => {
                    trace!(
                        ?unresolved,
                        ?resolved_from,
                        addr,
                        "Resolved to memory address"
                    );
                    set_integer_global(store, &key.import_name, global, addr)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Memory(addr),
                    );
                }

                (
                    UnresolvedGlobal::Mem(key, global),
                    Ok((PartiallyResolvedExport::Tls { offset, final_addr }, resolved_from)),
                ) => {
                    trace!(
                        ?unresolved,
                        ?resolved_from,
                        offset,
                        final_addr,
                        "Resolved to TLS address"
                    );
                    set_integer_global(store, &key.import_name, global, final_addr)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Tls {
                            resolved_from,
                            offset,
                        },
                    );
                }

                (
                    UnresolvedGlobal::Func(key, global),
                    Ok((PartiallyResolvedExport::Function(func), resolved_from)),
                ) => {
                    let func_handle = self
                        .append_to_function_table(store, func)
                        .map_err(LinkError::TableAllocationError)?;
                    trace!(
                        ?unresolved,
                        ?resolved_from,
                        function_table_index = ?func_handle,
                        "Resolved to function pointer"
                    );
                    set_integer_global(store, &key.import_name, global, func_handle as u64)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::FunctionPointer {
                            resolved_from,
                            function_table_index: func_handle,
                        },
                    );
                }

                // Expected memory address, resolved function or vice-versa
                (_, Ok(_)) => {
                    return Err(LinkError::UnresolvedGlobal(
                        unresolved.import_module().to_string(),
                        key.import_name.clone(),
                        Box::new(ResolveError::MissingExport),
                    ))
                }

                // Missing weak symbols get resolved to a null address
                (_, Err(ResolveError::MissingExport)) if is_weak => {
                    trace!(?unresolved, "Weak global not found");
                    set_integer_global(store, &key.import_name, unresolved.global(), 0)?;
                    linker_state.symbol_resolution_records.insert(
                        SymbolResolutionKey::Needed(key.clone()),
                        SymbolResolutionResult::Memory(0),
                    );
                }

                (_, Err(e)) => {
                    return Err(LinkError::UnresolvedGlobal(
                        "GOT.mem".to_string(),
                        key.import_name.clone(),
                        Box::new(e),
                    ))
                }
            }
        }

        Ok(())
    }
}

async fn locate_module(
    module_path: &Path,
    library_path: Option<&[impl AsRef<Path>]>,
    fs: &WasiFs,
) -> Result<(PathBuf, OwnedBuffer), LinkError> {
    async fn try_load(
        fs: &WasiFsRoot,
        path: impl AsRef<Path>,
    ) -> Result<(PathBuf, OwnedBuffer), FsError> {
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

        let buf = if let Some(buf) = file.as_owned_buffer() {
            buf
        } else {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await?;
            OwnedBuffer::from(buf)
        };

        Ok((path.as_ref().to_owned(), buf))
    }

    if module_path.is_absolute() {
        trace!(?module_path, "Locating module with absolute path");
        try_load(&fs.root_fs, module_path).await.map_err(|e| {
            LinkError::SharedLibraryMissing(
                module_path.to_string_lossy().into_owned(),
                LocateModuleError::Single(e),
            )
        })
    } else if module_path.components().count() > 1 {
        trace!(?module_path, "Locating module with relative path");
        try_load(
            &fs.root_fs,
            fs.relative_path_to_absolute(module_path.to_string_lossy().into_owned()),
        )
        .await
        .map_err(|e| {
            LinkError::SharedLibraryMissing(
                module_path.to_string_lossy().into_owned(),
                LocateModuleError::Single(e),
            )
        })
    } else {
        // Go through all dynamic library lookup paths
        // Note: a path without a slash does *not* look at the current directory. This is by design.

        // TODO: implement RUNPATH once it's supported by clang and wasmparser
        // TODO: support $ORIGIN and ${ORIGIN} in RUNPATH

        trace!(
            ?module_path,
            "Locating module by name in default runtime path"
        );
        let search_paths = library_path
            .iter()
            .flat_map(|paths| paths.iter().map(AsRef::as_ref))
            // Add default runtime paths
            .chain(DEFAULT_RUNTIME_PATH.iter().map(Path::new));

        let mut errors: Vec<(PathBuf, FsError)> = Vec::new();
        for path in search_paths {
            let full_path = path.join(module_path);
            trace!(?module_path, full_path = ?full_path, "Searching module");
            match try_load(&fs.root_fs, &full_path).await {
                Ok(ret) => {
                    trace!(?module_path, full_path = ?ret.0, "Located module");
                    return Ok(ret);
                }
                Err(e) => errors.push((full_path, e)),
            };
        }

        trace!(?module_path, "Failed to locate module");
        Err(LinkError::SharedLibraryMissing(
            module_path.to_string_lossy().into_owned(),
            LocateModuleError::Multiple(errors),
        ))
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
    let mut import_metadata = HashMap::new();
    let mut export_metadata = HashMap::new();

    for subsection in reader {
        let subsection = subsection?;
        match subsection {
            wasmparser::Dylink0Subsection::MemInfo(m) => {
                mem_info = Some(m);
            }

            wasmparser::Dylink0Subsection::Needed(n) => {
                needed = Some(n.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            }

            wasmparser::Dylink0Subsection::ImportInfo(i) => {
                for i in i {
                    import_metadata.insert((i.module.to_owned(), i.field.to_owned()), i.flags);
                }
            }

            wasmparser::Dylink0Subsection::ExportInfo(e) => {
                for e in e {
                    export_metadata.insert(e.name.to_owned(), e.flags);
                }
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
        import_metadata,
        export_metadata,
    })
}

fn get_integer_global_type_from_import(import: &ImportType) -> Result<GlobalType, LinkError> {
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

    Ok(*ty)
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

fn call_initialization_function<Ret: WasmTypeList>(
    instance: &Instance,
    store: &mut impl AsStoreMut,
    name: &str,
) -> Result<Option<Ret>, LinkError> {
    match instance.exports.get_typed_function::<(), Ret>(store, name) {
        Ok(f) => {
            let ret = f
                .call(store)
                .map_err(|e| LinkError::InitFunctionFailed(name.to_string(), e))?;
            Ok(Some(ret))
        }
        Err(ExportError::Missing(_)) => Ok(None),
        Err(ExportError::IncompatibleType) => {
            Err(LinkError::InitFuncWithInvalidSignature(name.to_string()))
        }
    }
}

#[cfg(test)]
mod memory_allocator_tests {
    use wasmer::{Engine, Memory, Store};

    use super::MemoryAllocator;

    const WASM_PAGE_SIZE: u32 = wasmer::WASM_PAGE_SIZE as u32;

    #[test]
    fn test_memory_allocator() {
        let engine = Engine::default();
        let mut store = Store::new(engine);
        let memory = Memory::new(
            &mut store,
            wasmer::MemoryType {
                minimum: wasmer::Pages(2),
                maximum: None,
                shared: true,
            },
        )
        .unwrap();
        let mut allocator = MemoryAllocator::new();

        // Small allocation in new page
        let addr = allocator.allocate(&memory, &mut store, 24, 4).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE);
        assert_eq!(memory.grow(&mut store, 0).unwrap().0, 3);

        // Small allocation in existing page
        let addr = allocator.allocate(&memory, &mut store, 16, 4).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE + 24);

        // Small allocation in existing page, with bigger alignment
        let addr = allocator.allocate(&memory, &mut store, 64, 32).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE + 64);
        // Should still have 3 pages
        assert_eq!(memory.grow(&mut store, 0).unwrap().0, 3);

        // Big allocation in new pages
        let addr = allocator
            .allocate(&memory, &mut store, 2 * WASM_PAGE_SIZE + 256, 1024)
            .unwrap();
        assert_eq!(addr, WASM_PAGE_SIZE * 3);
        assert_eq!(memory.grow(&mut store, 0).unwrap().0, 6);

        // Small allocation with multiple empty pages
        // page 2 has 128 bytes allocated, page 5 has 256, allocation should go
        // to page 5 (we should allocate from the page with the least free space)
        let addr = allocator
            .allocate(&memory, &mut store, 1024 * 63, 64)
            .unwrap();
        assert_eq!(addr, 5 * WASM_PAGE_SIZE + 256);

        // Another small allocation, but this time it won't fit on page 5
        let addr = allocator.allocate(&memory, &mut store, 4096, 512).unwrap();
        assert_eq!(addr, 2 * WASM_PAGE_SIZE + 512);
    }
}
