use super::*;
use crate::{
    capture_snapshot,
    os::task::thread::WasiMemoryLayout,
    runtime::task_manager::{TaskWasm, TaskWasmRunProperties},
    syscalls::*,
    WasiThreadHandle,
};

use wasmer::Memory;
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

    // We extract the memory which will be passed to the thread
    let thread_memory = env.memory_clone();

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

    // We capture some local variables
    let state = env.state.clone();
    let mut thread_env = env.clone();
    thread_env.thread = thread_handle.as_thread();
    thread_env.layout = layout;
    thread_env.enable_deep_sleep = thread_env.capable_of_deep_sleep();

    // This next function gets a context for the local thread and then
    // calls into the process
    let mut execute_module = {
        let thread_handle = thread_handle;
        move |ctx: WasiFunctionEnv, mut store: Store| {
            // Call the thread
            call_module::<M>(ctx, store, start_ptr_offset, thread_handle, None)
        }
    };

    // If the process does not export a thread spawn function then obviously
    // we can't spawn a background thread
    if env.inner().thread_spawn.is_none() {
        warn!("thread failed - the program does not export a `wasi_thread_start` function");
        return Errno::Notcapable;
    }
    let thread_module = env.inner().module_clone();
    let snapshot = capture_snapshot(&mut ctx.as_store_mut());
    let spawn_type =
        crate::runtime::SpawnMemoryType::ShareMemory(thread_memory, ctx.as_store_ref());

    // Write the thread ID to the return value
    let memory = ctx.data().memory_view(&ctx);
    wasi_try_mem!(ret_tid.write(&memory, thread_id));

    // Now spawn a thread
    trace!("threading: spawning background thread");
    let run = move |props: TaskWasmRunProperties| {
        execute_module(props.ctx, props.store);
    };
    wasi_try!(tasks
        .task_wasm(
            TaskWasm::new(Box::new(run), thread_env, thread_module, false)
                .with_snapshot(&snapshot)
                .with_memory(spawn_type)
        )
        .map_err(|err| { Into::<Errno>::into(err) }));

    // Success
    Errno::Success
}

/// Calls the module
fn call_module<M: MemorySize>(
    mut ctx: WasiFunctionEnv,
    mut store: Store,
    start_ptr_offset: M::Offset,
    thread_handle: Arc<WasiThreadHandle>,
    rewind_state: Option<(RewindState, Result<(), Errno>)>,
) -> u32 {
    let env = ctx.data(&store);
    let tasks = env.tasks().clone();

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
    if let Some((mut rewind_state, trigger_res)) = rewind_state {
        if let Err(exit_code) =
            rewind_state.rewinding_finish::<M>(ctx.env.clone().into_mut(&mut store), trigger_res)
        {
            return exit_code.raw() as u32;
        }
        let res = rewind::<M>(
            ctx.env.clone().into_mut(&mut store),
            rewind_state.memory_stack,
            rewind_state.rewind_stack,
            rewind_state.store_data,
        );
        if res != Errno::Success {
            return res as u32;
        }
    }

    // Now invoke the module
    let ret = call_module_internal(&ctx, &mut store);

    // If it went to deep sleep then we need to handle that
    match ret {
        Ok(ret) => {
            // Frees the handle so that it closes
            drop(thread_handle);
            ret
        }
        Err(deep) => {
            // Create the callback that will be invoked when the thread respawns after a deep sleep
            let rewind = deep.rewind;
            let respawn = {
                let tasks = tasks.clone();
                move |ctx, store, trigger_res| {
                    // Call the thread
                    call_module::<M>(
                        ctx,
                        store,
                        start_ptr_offset,
                        thread_handle,
                        Some((rewind, trigger_res)),
                    );
                }
            };

            /// Spawns the WASM process after a trigger
            tasks.resume_wasm_after_poller(Box::new(respawn), ctx, store, deep.trigger);
            Errno::Unknown as u32
        }
    }
}
