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
//! for each group via [`Memory::share_and_detach`] +
//! [`DetachedMemory::attach`](wasmer::DetachedMemory::attach). Also, when placing a symbol
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

mod dylink;
mod error;
mod instance_group;
mod internal_types;
mod linker_state;
mod locator;
mod memory_allocator;
mod sync;
mod types;
mod wasm_utils;

pub use dylink::*;
pub use error::*;
pub use types::*;

use instance_group::*;
use internal_types::*;
use linker_state::*;
use locator::*;
use memory_allocator::*;
use sync::*;
use wasm_utils::*;

use std::{
    collections::{BTreeMap, HashMap},
    ops::DerefMut,
    path::Path,
    sync::{Arc, Mutex, MutexGuard, atomic::Ordering},
};

use bus::Bus;
use tracing::trace;
use wasmer::{AsStoreMut, Engine, FunctionEnvMut, Instance, Memory, Module, StoreMut, Tag, Type};
use wasmer_wasix_types::wasix::WasiMemoryLayout;

use crate::{WasiEnv, WasiFunctionEnv, WasiModuleTreeHandles, import_object_for_all_wasi_versions};

use super::WasiModuleInstanceHandles;

// Module handle 1 is always the main module. Side modules get handles starting from the next one after the main module.
pub static MAIN_MODULE_HANDLE: ModuleHandle = ModuleHandle(1);
static INVALID_MODULE_HANDLE: ModuleHandle = ModuleHandle(u32::MAX);

// Need to keep the zeroth index null to catch null function pointers at runtime
static MAIN_MODULE_TABLE_BASE: u64 = 1;

/// The linker is responsible for loading and linking dynamic modules at runtime,
/// and managing the shared memory and indirect function table.
/// Each linker instance represents a specific instance group. Cloning a linker
/// instance does *not* create a new instance group though; the clone will refer
/// to the same group as the original.
#[derive(Clone)]
pub struct Linker {
    shared: LinkerShared,
    instance_group_state: Arc<Mutex<Option<InstanceGroupState>>>,
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
        engine: Engine,
        main_module: &Module,
        store: &mut StoreMut<'_>,
        memory: Option<Memory>,
        func_env: &mut WasiFunctionEnv,
        stack_size: u64,
        ld_library_path: &[&Path],
    ) -> Result<(Self, LinkedMainModule), LinkError> {
        let dylink_section = parse_dylink0_section(main_module)?;

        trace!(?dylink_section, "Loading main module");

        let mut imports = import_object_for_all_wasi_versions(main_module, store, &func_env.env);

        let function_table_type = main_module_function_table_type(main_module)?;

        let expected_table_length =
            dylink_section.mem_info.table_size + MAIN_MODULE_TABLE_BASE as u32;
        let indirect_function_table =
            create_indirect_function_table(store, function_table_type, expected_table_length)?;

        // Give modules a non-zero memory base, since we don't want
        // any valid pointers to point to the zero address
        let memory_base = 2u64.pow(dylink_section.mem_info.memory_alignment);

        let memory_type = main_module_memory_type(main_module)?;

        let memory = match memory {
            Some(m) => m,
            None => Memory::new(store, memory_type)?,
        };

        let stack_low = {
            let data_end = memory_base + dylink_section.mem_info.memory_size as u64;
            if !data_end.is_multiple_of(1024) {
                data_end + 1024 - (data_end % 1024)
            } else {
                data_end
            }
        };

        if !stack_size.is_multiple_of(1024) {
            panic!("Stack size must be 1024-bit aligned");
        }

        let stack_high = stack_low + stack_size;

        // Allocate memory for the stack. This does not need to go through the memory allocator
        // because it's always placed directly after the main module's data
        memory.grow_at_least(store, stack_high)?;

        trace!(
            memory_pages = ?memory.grow(store, 0).unwrap(),
            memory_base,
            stack_low,
            stack_high,
            "Memory layout"
        );

        let stack_pointer = create_main_stack_pointer_global(store, main_module, stack_high)?;

        let c_longjmp = Tag::new(store, vec![Type::I32]);
        let cpp_exception = Tag::new(store, vec![Type::I32]);

        let mut barrier_tx = Bus::new(1);
        let barrier_rx = barrier_tx.add_rx();
        let mut operation_tx = Bus::new(1);
        let operation_rx = operation_tx.add_rx();

        let mut instance_group = InstanceGroupState {
            main_instance: None,
            // The TLS base for the main instance is determined by reading the
            // `__tls_base` global export from the instance after instantiation.
            main_instance_tls_base: None,
            side_instances: HashMap::new(),
            stack_pointer,
            memory: memory.clone(),
            indirect_function_table: indirect_function_table.clone(),
            c_longjmp,
            cpp_exception,
            recv_pending_operation_barrier: barrier_rx,
            recv_pending_operation: operation_rx,
        };

        let mut linker_state = LinkerState {
            engine,
            main_module: main_module.clone(),
            main_module_dylink_info: dylink_section,
            main_module_memory_base: memory_base,
            side_modules: BTreeMap::new(),
            side_modules_by_name: HashMap::new(),
            next_module_handle: MAIN_MODULE_HANDLE.0 + 1,
            memory_allocator: MemoryAllocator::new(),
            allocated_closure_functions: BTreeMap::new(),
            available_closure_functions: Vec::new(),
            heap_base: stack_high,
            symbol_resolution_records: HashMap::new(),
            send_pending_operation_barrier: barrier_tx,
            send_pending_operation: operation_tx,
        };

        let mut link_state = InProgressLinkState::default();

        let well_known_imports = [
            ("env", "__memory_base", memory_base),
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

        let tls_base = get_tls_base_export(&main_instance, store)?;
        instance_group.main_instance_tls_base = tls_base;

        let runtime_path = linker_state.main_module_dylink_info.runtime_path.clone();
        for needed in linker_state.main_module_dylink_info.needed.clone() {
            // A successful load_module will add the module to the side_modules list,
            // from which symbols can be resolved in the following call to
            // guard.resolve_imports.
            trace!(name = needed, "Loading module needed by main");
            let wasi_env = func_env.data(store);
            linker_state.load_module_tree(
                DlModuleSpec::FileSystem {
                    module_spec: Path::new(needed.as_str()),
                    ld_library_path,
                },
                &mut link_state,
                &wasi_env.runtime,
                &wasi_env.state,
                runtime_path.as_ref(),
                // HACK: The main module doesn't have to exist in the virtual FS at all; e.g.
                // if one runs `wasmer ../module.wasm --volume .`, we won't have access to the
                // main module's folder within the virtual FS. This is why we're picking PWD
                // as the $ORIGIN of the main module, which should at least be slightly
                // sensible. The `main.wasm` file name will be stripped and only the `./`
                // will be taken into account by `locate_module`.
                Some(Path::new("./main.wasm")),
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
            shared: LinkerShared::new(linker_state),
            instance_group_state: Arc::new(Mutex::new(Some(instance_group))),
        };

        let stack_layout = WasiMemoryLayout {
            stack_lower: stack_low,
            stack_upper: stack_high,
            stack_size: stack_high - stack_low,
            guard_size: 0,
            tls_base,
        };
        let module_handles = WasiModuleTreeHandles::Dynamic {
            linker: linker.clone(),
            main_module_instance_handles: WasiModuleInstanceHandles::new(
                memory.clone(),
                store,
                main_instance.clone(),
                Some(indirect_function_table.clone()),
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

        {
            trace!(?link_state, "Finalizing linking of main module");

            let mut group_guard = linker.instance_group_state.lock().unwrap();
            unsafe {
                linker.shared.bootstrap_exclusive_write_then(|ls| {
                    let group_state = group_guard.as_mut().unwrap();
                    group_state.finalize_pending_globals(
                        ls,
                        store,
                        &link_state.unresolved_globals,
                    )?;

                    trace!("Calling data relocator function for main module");
                    call_initialization_function::<()>(
                        &main_instance,
                        store,
                        "__wasm_apply_data_relocs",
                    )?;
                    call_initialization_function::<()>(
                        &main_instance,
                        store,
                        "__wasm_apply_tls_relocs",
                    )?;

                    linker.initialize_new_modules(group_guard, store, link_state)
                })?;
            }
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

    /// This method gathers all necessary data from a parent thread's
    /// environment, so a child thread can later call [`Self::create_instance_group`]
    /// and have its own instance group, letting it take part in dynamic linking.
    /// This two-part process is needed because the parent and child each have
    /// their own [`Store`], and [`Store`]s are not `Send`.
    pub fn prepare_for_instance_group(
        &self,
        parent_ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<PreparedInstanceGroupData, LinkError> {
        trace!("Preparing for new instance group");

        lock_instance_group_state!(
            parent_group_state_guard,
            parent_group_state,
            self,
            LinkError::InstanceGroupIsDead
        );

        // Lease topology only: parent does not mutate shared `LinkerState` here; the child takes
        // the blocking write in `create_instance_group` while holding the moved token.
        let env = parent_ctx.as_ref();
        let mut store = parent_ctx.as_store_mut();
        let topology_token =
            self.shared
                .acquire_topology_token(parent_group_state, &mut store, &env)?;

        let parent_store = parent_ctx.as_store_mut();

        let memory = parent_group_state
            .memory
            .as_shared(&parent_store)
            .ok_or_else(|| LinkError::MemoryNotShared)?;

        let indirect_function_table_type =
            parent_group_state.indirect_function_table.ty(&parent_store);

        let expected_table_length = parent_group_state
            .indirect_function_table
            .size(&parent_store);

        Ok(PreparedInstanceGroupData {
            linker_shared: self.shared.clone(),
            topology_token,
            memory,
            indirect_function_table_type,
            expected_table_length,
        })
    }

    pub(crate) fn do_pending_link_operations(
        &self,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fast: bool,
    ) -> Result<(), LinkError> {
        if !self.shared.dl_operation_pending_load(if fast {
            Ordering::Relaxed
        } else {
            Ordering::SeqCst
        }) {
            return Ok(());
        }

        lock_instance_group_state!(guard, group_state, self, LinkError::InstanceGroupIsDead);

        let env = ctx.as_ref();
        let mut store = ctx.as_store_mut();
        self.shared
            .do_pending_link_operations_internal(group_state, &mut store, &env)
    }

    pub fn create_instance_group(
        prepared_instance_group_data: PreparedInstanceGroupData,
        store: &mut StoreMut<'_>,
        func_env: &mut WasiFunctionEnv,
    ) -> Result<(Self, LinkedMainModule), LinkError> {
        trace!("Spawning new instance group");

        let PreparedInstanceGroupData {
            linker_shared,
            topology_token,
            memory,
            indirect_function_table_type,
            expected_table_length,
        } = prepared_instance_group_data;

        let (topology_hold, mut ls_write) =
            linker_shared.write_linker_state_blocking_holding_topology(topology_token);

        let main_module = ls_write.main_module.clone();

        let mut imports = import_object_for_all_wasi_versions(&main_module, store, &func_env.env);

        let memory = memory.attach(store);

        let indirect_function_table = create_indirect_function_table(
            store,
            indirect_function_table_type,
            expected_table_length,
        )?;

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

        // WASIX threads initialize their own stack pointer global in wasi_thread_start,
        // so no need to initialize it to a value here.
        let stack_pointer = create_main_stack_pointer_global(store, &main_module, 0)?;

        let c_longjmp = Tag::new(store, vec![Type::I32]);
        let cpp_exception = Tag::new(store, vec![Type::I32]);

        let barrier_rx = ls_write.send_pending_operation_barrier.add_rx();
        let operation_rx = ls_write.send_pending_operation.add_rx();

        let mut instance_group = InstanceGroupState {
            main_instance: None,
            main_instance_tls_base: Some(tls_base),
            side_instances: HashMap::new(),
            stack_pointer,
            memory: memory.clone(),
            indirect_function_table: indirect_function_table.clone(),
            c_longjmp,
            cpp_exception,
            recv_pending_operation_barrier: barrier_rx,
            recv_pending_operation: operation_rx,
        };

        let mut pending_resolutions = PendingResolutionsFromLinker::default();

        let well_known_imports = [
            ("env", "__memory_base", ls_write.main_module_memory_base),
            ("env", "__table_base", MAIN_MODULE_TABLE_BASE),
            ("GOT.mem", "__stack_high", stack_high),
            ("GOT.mem", "__stack_low", stack_low),
            ("GOT.mem", "__heap_base", ls_write.heap_base),
        ];

        trace!("Populating imports object for new instance group's main instance");
        instance_group.populate_imports_from_linker(
            MAIN_MODULE_HANDLE,
            &ls_write,
            store,
            &main_module,
            &mut imports,
            &func_env.env,
            &well_known_imports,
            &mut pending_resolutions,
        )?;

        let main_instance = Instance::new(store, &main_module, &imports)?;

        instance_group.main_instance = Some(main_instance.clone());

        for side in &ls_write.side_modules {
            trace!(module_handle = ?side.0, "Instantiating existing side module");
            instance_group.instantiate_side_module_from_linker(
                &ls_write,
                store,
                &func_env.env,
                *side.0,
                &mut pending_resolutions,
            )?;
        }

        trace!("Finalizing pending functions");
        instance_group.finalize_pending_resolutions_from_linker(&pending_resolutions, store)?;

        trace!("Applying externally-requested function table entries");
        instance_group.apply_requested_symbols_from_linker(store, &ls_write)?;

        drop(ls_write);
        drop(topology_hold);

        let linker = Self {
            shared: linker_shared,
            instance_group_state: Arc::new(Mutex::new(Some(instance_group))),
        };

        let module_handles = WasiModuleTreeHandles::Dynamic {
            linker: linker.clone(),
            main_module_instance_handles: WasiModuleInstanceHandles::new(
                memory.clone(),
                store,
                main_instance.clone(),
                Some(indirect_function_table.clone()),
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
                let linker_state = self.shared.write_linker_state(group_state, ctx)?;
                guard.take();
                drop(linker_state);

                trace!("Instance group shut down");

                Ok(())
            }
        }
    }

    /// Allocate a index for a closure in the indirect function table
    pub fn allocate_closure_index(
        &self,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<u32, LinkError> {
        lock_instance_group_state!(
            group_state_guard,
            group_state,
            self,
            LinkError::InstanceGroupIsDead
        );
        let mut linker_state = self.shared.write_linker_state(group_state, ctx)?;

        // Use a previously allocated slot if possible
        if let Some(function_index) = linker_state.available_closure_functions.pop() {
            linker_state
                .allocated_closure_functions
                .insert(function_index, true);
            return Ok(function_index);
        }

        drop(linker_state);

        let (topology_token, mut linker_state) = self
            .shared
            .write_linker_state_with_topology(group_state, ctx)?;

        let mut store = ctx.as_store_mut();

        // Another group may have refilled slots while we released the linker lock.
        if let Some(function_index) = linker_state.available_closure_functions.pop() {
            linker_state
                .allocated_closure_functions
                .insert(function_index, true);
            drop(linker_state);
            drop(topology_token);
            return Ok(function_index);
        }

        // Allocate more closures than we need to reduce the number of sync operations
        const CLOSURE_ALLOCATION_SIZE: u32 = 100;

        let function_index = group_state
            .allocate_function_table(&mut store, CLOSURE_ALLOCATION_SIZE, 0)
            .map_err(LinkError::TableAllocationError)? as u32;

        linker_state
            .available_closure_functions
            .reserve(CLOSURE_ALLOCATION_SIZE as usize - 1);
        for i in 1..CLOSURE_ALLOCATION_SIZE {
            linker_state
                .available_closure_functions
                .push(function_index + i);
            linker_state
                .allocated_closure_functions
                .insert(function_index + i, false);
        }
        linker_state
            .allocated_closure_functions
            .insert(function_index, true);

        self.shared.synchronize_link_operation(
            topology_token,
            DlOperation::AllocateFunctionTable {
                index: function_index,
                size: CLOSURE_ALLOCATION_SIZE,
            },
            linker_state,
            group_state,
            &ctx.data().process,
            ctx.data().tid(),
        );

        Ok(function_index)
    }

    /// Remove a previously allocated slot for a closure in the indirect function table
    ///
    /// After calling this it is undefined behavior to call the function at the given index.
    pub fn free_closure_index(
        &self,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        function_id: u32,
    ) -> Result<(), LinkError> {
        lock_instance_group_state!(
            group_state_guard,
            group_state,
            self,
            LinkError::InstanceGroupIsDead
        );
        let mut linker_state = self.shared.write_linker_state(group_state, ctx)?;

        let Some(entry) = linker_state
            .allocated_closure_functions
            .get_mut(&function_id)
        else {
            // Not allocated
            return Ok(());
        };
        if !*entry {
            // Not used
            return Ok(());
        }

        *entry = false;
        linker_state.available_closure_functions.push(function_id);
        Ok(())
    }

    /// Check if an indirect_function_table entry is reserved for closures.
    /// Returns false if the entry is not reserved for closures.
    /// Requires a FunctionEnvMut because pending DL operations should always
    /// be processed before acquiring any lock on the linker.
    // TODO: we can cache this information within the group state so we don't
    // need a write lock on the linker state here
    pub fn is_closure(
        &self,
        function_id: u32,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<bool, LinkError> {
        // If we can get a read lock on the linker state, do it
        if let Ok(linker_state) = self.shared.try_read_linker_state() {
            return Ok(linker_state
                .allocated_closure_functions
                .contains_key(&function_id));
        }

        // Otherwise, fall back to the path where we apply DL ops and acquire
        // a write lock afterwards
        lock_instance_group_state!(
            group_state_guard,
            group_state,
            self,
            LinkError::InstanceGroupIsDead
        );
        let linker_state = self.shared.write_linker_state(group_state, ctx)?;
        Ok(linker_state
            .allocated_closure_functions
            .contains_key(&function_id))
    }

    /// Loads a side module from the given path, linking it against the existing module tree
    /// and instantiating it. Symbols from the module can then be retrieved by calling
    /// [`Linker::resolve_export`].
    pub fn load_module(
        &self,
        module_spec: DlModuleSpec,
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ) -> Result<ModuleHandle, LinkError> {
        trace!(?module_spec, "Loading module");

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
        let (topology_token, mut linker_state) = self
            .shared
            .write_linker_state_with_topology(group_state, ctx)?;

        let mut link_state = InProgressLinkState::default();
        let env = ctx.as_ref();
        let mut store = ctx.as_store_mut();

        trace!("Loading module tree for requested module");
        let wasi_env = env.as_ref(&store);
        let runtime_path: &[String] = &[];
        let module_handle = linker_state.load_module_tree(
            module_spec,
            &mut link_state,
            &wasi_env.runtime,
            &wasi_env.state,
            runtime_path,          // No runtime path when loading a module via dlopen
            Option::<&Path>::None, // Empty runtime path means we don't need the module's path either
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

            self.shared.synchronize_link_operation(
                topology_token,
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

        self.initialize_new_modules(group_state_guard, store, link_state)
    }

    fn initialize_new_modules(
        &self,
        // Take ownership of the guard and drop it ourselves to ensure no deadlock can happen
        mut group_state_guard: MutexGuard<'_, Option<InstanceGroupState>>,
        store: &mut impl AsStoreMut,
        link_state: InProgressLinkState,
    ) -> Result<(), LinkError> {
        let group_state = group_state_guard.as_mut().unwrap();

        let new_instances = link_state
            .new_modules
            .iter()
            .map(|m| group_state.side_instances[&m.handle].instance.clone())
            .collect::<Vec<_>>();

        // The instance group must be unlocked for the next step, since modules may need to resolve
        // stub functions and that requires a lock on the instance group's state
        drop(group_state_guard);

        // These functions are exported from PIE executables, and need to be run before calling
        // _initialize or _start. More info:
        // https://github.com/WebAssembly/tool-conventions/blob/main/DynamicLinking.md
        trace!("Calling data relocation functions");
        for instance in &new_instances {
            call_initialization_function::<()>(instance, store, "__wasm_apply_data_relocs")?;
            call_initialization_function::<()>(instance, store, "__wasm_apply_tls_relocs")?;
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

        let resolution_key = SymbolResolutionKey::Requested {
            resolve_from: module_handle,
            name: symbol.to_string(),
        };

        lock_instance_group_state!(guard, group_state, self, ResolveError::InstanceGroupIsDead);

        if let Ok(linker_state) = self.shared.try_read_linker_state()
            && let Some(resolution) = linker_state.symbol_resolution_records.get(&resolution_key)
        {
            trace!(?resolution, "Already have a resolution for this symbol");
            match resolution {
                SymbolResolutionResult::FunctionPointer {
                    function_table_index: addr,
                    ..
                } => {
                    return Ok(ResolvedExport::Function {
                        func_ptr: *addr as u64,
                    });
                }
                SymbolResolutionResult::Memory(addr) => {
                    return Ok(ResolvedExport::Global { data_ptr: *addr });
                }
                SymbolResolutionResult::Tls {
                    resolved_from,
                    offset,
                } => {
                    let Some(tls_base) = group_state.tls_base(*resolved_from) else {
                        return Err(ResolveError::NoTlsBaseGlobalExport);
                    };
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

        let (topology_token, mut linker_state) = self
            .shared
            .write_linker_state_with_topology(group_state, ctx)?;

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

                self.shared.synchronize_link_operation(
                    topology_token,
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
        // If we can get a read lock on the linker state, do it
        if let Ok(linker_state) = self.shared.try_read_linker_state() {
            return Ok(linker_state.side_modules.contains_key(&handle));
        }

        // Otherwise, fall back to the path where we apply DL ops and acquire
        // a write lock afterwards
        lock_instance_group_state!(guard, group_state, self, LinkError::InstanceGroupIsDead);
        let linker_state = self.shared.write_linker_state(group_state, ctx)?;
        Ok(linker_state.side_modules.contains_key(&handle))
    }
}
