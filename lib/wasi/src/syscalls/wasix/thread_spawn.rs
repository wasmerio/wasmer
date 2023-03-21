use super::*;
use crate::{
    os::task::thread::WasiMemoryLayout, runtime::task_manager::TaskResumeAction, syscalls::*,
    WasiThreadHandle,
};

use wasmer::{vm::VMMemory, MemoryType};
use wasmer_wasix_types::wasi::ThreadStart;

/// ### `thread_spawn()`
/// Creates a new thread by spawning that shares the same
/// memory address space, file handles and main event loops.
/// The function referenced by the fork call must be
/// exported by the web assembly process.
///
/// ## Parameters
///
/// * `name` - Name of the function that will be invoked as a new thread
/// * `user_data` - User data that will be supplied to the function when its called
/// * `reactor` - Indicates if the function will operate as a reactor or
///   as a normal thread. Reactors will be repeatable called
///   whenever IO work is available to be processed.
///
/// ## Return
///
/// Returns the thread index of the newly created thread
/// (indices always start from zero)
#[instrument(level = "debug", skip_all, fields(user_data, reactor, tid = field::Empty), ret)]
pub fn thread_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    // Now we use the environment and memory references
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let runtime = env.runtime.clone();
    let tasks = env.tasks().clone();
    let start_ptr_offset = start_ptr.offset();

    // Read the properties about the stack which we will use for asyncify
    let layout = {
        let start = wasi_try_mem!(start_ptr.read(&memory));
        let stack_upper: u64 = wasi_try!(start.stack_upper.try_into().map_err(|_| Errno::Overflow));
        let stack_size: u64 = wasi_try!(start.stack_size.try_into().map_err(|_| Errno::Overflow));
        let guard_size: u64 = wasi_try!(start.guard_size.try_into().map_err(|_| Errno::Overflow));
        let tls_base: u64 = wasi_try!(start.tls_base.try_into().map_err(|_| Errno::Overflow));
        let stack_lower = stack_upper - stack_size;

        tracing::trace!(%stack_upper, %stack_lower, %stack_size, %guard_size, %tls_base);

        WasiMemoryLayout {
            stack_upper,
            stack_lower,
            guard_size,
            stack_size,
        }
    };

    // Create the handle that represents this thread
    let mut thread_handle = match env.process.new_thread() {
        Ok(h) => Arc::new(h),
        Err(err) => {
            error!(
                stack_base = layout.stack_lower,
                "failed to create thread handle",
            );
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Errno::Access;
        }
    };
    let thread_id: Tid = thread_handle.id().into();
    Span::current().record("tid", thread_id);

    // We need a copy of the process memory and a packaged store in order to
    // launch threads and reactors
    let thread_memory_ty = ctx.data().memory().ty(&ctx);
    let thread_memory = wasi_try!(ctx.data().memory().try_clone(&ctx).ok_or_else(|| {
        error!("failed - the memory could not be cloned");
        Errno::Notcapable
    }));

    let mut store = ctx.data().runtime.new_store();

    // We capture some local variables
    let state = env.state.clone();
    let mut wasi_env = env.duplicate();
    wasi_env.thread = thread_handle.as_thread();
    wasi_env.layout = layout;

    // If the environment is capable of doing it then we support deep sleeping
    wasi_env.enable_deep_sleep = env.capable_of_deep_sleep();
    tracing::trace!(enable_deep_sleep = wasi_env.enable_deep_sleep);

    // This next function gets a context for the local thread and then
    // calls into the process
    let mut execute_module = {
        let state = env.state.clone();
        let tasks = tasks.clone();
        let wasi_env = wasi_env.duplicate();
        let thread_handle = thread_handle.clone();
        move |mut store: Store, module: Module, mut memory: Option<VMMemory>| {
            // Now create the context and hook it up
            let ctx = match create_ctx(&mut store, &module, memory, wasi_env) {
                Ok(c) => c,
                Err(err) => {
                    return err as u32;
                }
            };

            // Call the thread
            call_module::<M>(
                ctx,
                store,
                module,
                tasks,
                start_ptr_offset,
                thread_memory_ty,
                thread_handle,
                None,
            )
        }
    };

    // If the process does not export a thread spawn function then obviously
    // we can't spawn a background thread
    if env.inner().thread_spawn.is_none() {
        warn!("thread failed - the program does not export a `wasi_thread_start` function");
        return Errno::Notcapable;
    }
    let spawn_type = crate::runtime::SpawnType::NewThread(thread_memory, thread_memory_ty);

    // Write the thread ID to the return value
    let memory = ctx.data().memory_view(&ctx);
    wasi_try_mem!(ret_tid.write(&memory, thread_id));

    // Now spawn a thread
    trace!("threading: spawning background thread");
    let thread_module = env.inner().instance.module().clone();
    let task = move |store, thread_module, thread_memory| {
        execute_module(store, thread_module, thread_memory);
    };
    wasi_try!(tasks
        .task_wasm(Box::new(task), store, thread_module, spawn_type)
        .map_err(|err| { Into::<Errno>::into(err) }));

    // Success
    Errno::Success
}

/// Calls the module
fn call_module<M: MemorySize>(
    env: WasiFunctionEnv,
    mut store: Store,
    module: Module,
    tasks: Arc<dyn VirtualTaskManager>,
    start_ptr_offset: M::Offset,
    thread_memory_ty: MemoryType,
    thread_handle: Arc<WasiThreadHandle>,
    rewind_state: Option<RewindState>,
) -> u32 {
    // This function calls into the module
    let call_module_internal = move |env: &WasiFunctionEnv, store: &mut Store| {
        // We either call the reactor callback or the thread spawn callback
        //trace!("threading: invoking thread callback (reactor={})", reactor);
        let spawn = env.data(&store).inner().thread_spawn.clone().unwrap();
        let tid = env.data(&store).tid();
        let call_ret = spawn.call(
            store,
            tid.raw().try_into().map_err(|_| Errno::Overflow).unwrap(),
            start_ptr_offset
                .try_into()
                .map_err(|_| Errno::Overflow)
                .unwrap(),
        );
        let mut ret = Errno::Success;
        if let Err(err) = call_ret {
            match err.downcast::<WasiError>() {
                Ok(WasiError::Exit(code)) => {
                    ret = if code.is_success() {
                        Errno::Success
                    } else {
                        Errno::Noexec
                    };
                }
                Ok(WasiError::DeepSleep(deep)) => {
                    trace!("entered a deep sleep");
                    return Err(deep);
                }
                Ok(WasiError::UnknownWasiVersion) => {
                    debug!("failed as wasi version is unknown",);
                    ret = Errno::Noexec;
                }
                Err(err) => {
                    debug!("failed with runtime error: {}", err);
                    ret = Errno::Noexec;
                }
            }
        }
        trace!("callback finished (ret={})", ret);

        // Clean up the environment
        env.cleanup(store, Some(ret.into()));

        // Return the result
        Ok(ret as u32)
    };

    // If we need to rewind then do so
    if let Some(rewind_state) = rewind_state {
        let ctx = env.env.clone().into_mut(&mut store);
        let res = rewind::<M>(
            ctx,
            rewind_state.memory_stack.freeze(),
            rewind_state.rewind_stack.freeze(),
            rewind_state.store_data,
        );
        if res != Errno::Success {
            return res as u32;
        }
    }

    // Now invoke the module
    let ret = call_module_internal(&env, &mut store);

    // If it went to deep sleep then we need to handle that
    match ret {
        Ok(ret) => ret,
        Err(deep) => {
            // Create the callback that will be invoked when the thread respawns after a deep sleep
            let rewind = deep.rewind;
            let respawn = {
                let env = env.clone();
                let tasks = tasks.clone();
                move |store, module| {
                    // Call the thread
                    call_module::<M>(
                        env,
                        store,
                        module,
                        tasks,
                        start_ptr_offset,
                        thread_memory_ty,
                        thread_handle,
                        Some(rewind),
                    );
                }
            };

            /// Spawns the WASM process after a trigger
            tasks.resume_wasm_after_poller(Box::new(respawn), store, module, env, deep.work);
            Errno::Unknown as u32
        }
    }
}

// This function takes in memory and a store and creates a context that
// can be used to call back into the process
fn create_ctx(
    store: &mut Store,
    module: &Module,
    mut memory: Option<VMMemory>,
    wasi_env: WasiEnv,
) -> Result<WasiFunctionEnv, Errno> {
    // Otherwise we need to create a new context under a write lock
    debug!("encountered a new caller - creating WASM execution context...");

    let memory = match memory.take() {
        Some(m) => m,
        None => {
            debug!("failed - memory can only be consumed once per context creation");
            return Err(Errno::Noexec);
        }
    };
    let memory = Memory::new_from_existing(store, memory);

    // Build the context object and import the memory
    let mut ctx = WasiFunctionEnv::new(store, wasi_env);
    let (mut import_object, init) = import_object_for_all_wasi_versions(module, store, &ctx.env);
    import_object.define("env", "memory", memory.clone());

    let instance = match Instance::new(store, module, &import_object) {
        Ok(a) => a,
        Err(err) => {
            error!("failed - create instance failed: {}", err);
            return Err(Errno::Noexec);
        }
    };

    init(&instance, &store).unwrap();

    // Set the current thread ID
    ctx.data_mut(store).inner = Some(WasiInstanceHandles::new(memory, &store, instance));
    Ok(ctx)
}
