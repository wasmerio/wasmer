use std::f32::consts::E;

use super::*;
#[cfg(feature = "journal")]
use crate::journal::JournalEffector;
use crate::{
    capture_store_snapshot,
    os::task::thread::WasiMemoryLayout,
    runtime::{
        task_manager::{TaskWasm, TaskWasmRunProperties},
        TaintReason,
    },
    syscalls::*,
    WasiThreadHandle,
};

use wasmer::Memory;
use wasmer_wasix_types::wasi::ThreadStart;

/// ### `thread_spawn()`
/// Creates a new thread by spawning that shares the same
/// memory address space, file handles and main event loops.
///
/// ## Parameters
///
/// * `start_ptr` - Pointer to the structure that describes the thread to be launched
/// * `ret_tid` - ID of the thread that was launched
///
/// ## Return
///
/// Returns the thread index of the newly created thread
/// (indices always start from the same value as `pid` and increments in steps)
//#[instrument(level = "debug", skip_all, ret)]
pub fn thread_spawn_v2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    // Create the thread
    let tid = wasi_try!(thread_spawn_internal_from_wasi(&mut ctx, start_ptr));

    // Success
    let memory = unsafe { ctx.data().memory_view(&ctx) };
    wasi_try_mem!(ret_tid.write(&memory, tid));
    Errno::Success
}

pub fn thread_spawn_internal_from_wasi<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
) -> Result<Tid, Errno> {
    // Now we use the environment and memory references
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let runtime = env.runtime.clone();
    let tasks = env.tasks().clone();
    let start_ptr_offset = start_ptr.offset();

    // Read the properties about the stack which we will use for asyncify
    let layout = {
        let start: ThreadStart<M> = start_ptr.read(&memory).map_err(mem_error_to_wasi)?;
        let stack_upper: u64 = start.stack_upper.try_into().map_err(|_| Errno::Overflow)?;
        let stack_size: u64 = start.stack_size.try_into().map_err(|_| Errno::Overflow)?;
        let guard_size: u64 = start.guard_size.try_into().map_err(|_| Errno::Overflow)?;
        let tls_base: u64 = start.tls_base.try_into().map_err(|_| Errno::Overflow)?;
        let stack_lower = stack_upper - stack_size;

        WasiMemoryLayout {
            stack_upper,
            stack_lower,
            guard_size,
            stack_size,
        }
    };
    tracing::trace!("spawn with layout {:?}", layout);

    // Create the handle that represents this thread
    let thread_start = ThreadStartType::ThreadSpawn {
        start_ptr: start_ptr_offset.into(),
    };
    let mut thread_handle = match env.process.new_thread(layout.clone(), thread_start) {
        Ok(h) => Arc::new(h),
        Err(err) => {
            error!(
                stack_base = layout.stack_lower,
                "failed to create thread handle",
            );
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Err(Errno::Access);
        }
    };
    let thread_id: Tid = thread_handle.id().into();
    Span::current().record("tid", thread_id);

    // Spawn the thread
    thread_spawn_internal_using_layout::<M>(ctx, thread_handle, layout, start_ptr_offset, None)?;

    // Success
    Ok(thread_id)
}

pub fn thread_spawn_internal_using_layout<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    thread_handle: Arc<WasiThreadHandle>,
    layout: WasiMemoryLayout,
    start_ptr_offset: M::Offset,
    rewind_state: Option<(RewindState, RewindResultType)>,
) -> Result<(), Errno> {
    // We extract the memory which will be passed to the thread
    let env = ctx.data();
    let tasks = env.tasks().clone();
    let thread_memory = unsafe { env.inner() }.memory_clone();

    // We capture some local variables
    let state = env.state.clone();
    let mut thread_env = env.clone();
    thread_env.thread = thread_handle.as_thread();
    thread_env.layout = layout;

    // TODO: Currently asynchronous threading does not work with multi
    //       threading in JS but it does work for the main thread. This will
    //       require more work to find out why.
    thread_env.enable_deep_sleep = if cfg!(feature = "js") {
        false
    } else {
        unsafe { env.capable_of_deep_sleep() }
    };

    // This next function gets a context for the local thread and then
    // calls into the process
    let mut execute_module = {
        let thread_handle = thread_handle;
        move |ctx: WasiFunctionEnv, mut store: Store| {
            // Call the thread
            call_module::<M>(ctx, store, start_ptr_offset, thread_handle, rewind_state)
        }
    };

    // If the process does not export a thread spawn function then obviously
    // we can't spawn a background thread
    if unsafe { env.inner() }.thread_spawn.is_none() {
        warn!("thread failed - the program does not export a `wasi_thread_start` function");
        return Err(Errno::Notcapable);
    }
    let thread_module = unsafe { env.inner() }.module_clone();
    let globals = capture_store_snapshot(&mut ctx.as_store_mut());
    let spawn_type =
        crate::runtime::SpawnMemoryType::ShareMemory(thread_memory, ctx.as_store_ref());

    // Now spawn a thread
    trace!("threading: spawning background thread");
    let run = move |props: TaskWasmRunProperties| {
        execute_module(props.ctx, props.store);
    };
    tasks
        .task_wasm(
            TaskWasm::new(Box::new(run), thread_env, thread_module, false)
                .with_globals(&globals)
                .with_memory(spawn_type),
        )
        .map_err(Into::<Errno>::into)?;

    // Success
    Ok(())
}

/// Calls the module
fn call_module<M: MemorySize>(
    mut ctx: WasiFunctionEnv,
    mut store: Store,
    start_ptr_offset: M::Offset,
    thread_handle: Arc<WasiThreadHandle>,
    rewind_state: Option<(RewindState, RewindResultType)>,
) -> Result<Tid, Errno> {
    let env = ctx.data(&store);
    let tasks = env.tasks().clone();

    // This function calls into the module
    let call_module_internal = move |env: &WasiFunctionEnv, store: &mut Store| {
        // We either call the reactor callback or the thread spawn callback
        //trace!("threading: invoking thread callback (reactor={})", reactor);
        let spawn = unsafe { env.data(&store).inner() }
            .thread_spawn
            .clone()
            .unwrap();
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
                        env.data(&store)
                            .runtime
                            .on_taint(TaintReason::NonZeroExitCode(code));
                        Errno::Noexec
                    };
                }
                Ok(WasiError::DeepSleep(deep)) => {
                    trace!("entered a deep sleep");
                    return Err(deep);
                }
                Ok(WasiError::UnknownWasiVersion) => {
                    debug!("failed as wasi version is unknown",);
                    env.data(&store)
                        .runtime
                        .on_taint(TaintReason::UnknownWasiVersion);
                    ret = Errno::Noexec;
                }
                Err(err) => {
                    debug!("failed with runtime error: {}", err);
                    env.data(&store)
                        .runtime
                        .on_taint(TaintReason::RuntimeError(err));
                    ret = Errno::Noexec;
                }
            }
        }
        trace!("callback finished (ret={})", ret);

        // Clean up the environment
        env.on_exit(store, Some(ret.into()));

        // Return the result
        Ok(ret as u32)
    };

    // If we need to rewind then do so
    if let Some((rewind_state, rewind_result)) = rewind_state {
        let mut ctx = ctx.env.clone().into_mut(&mut store);
        let res = rewind_ext::<M>(
            &mut ctx,
            Some(rewind_state.memory_stack),
            rewind_state.rewind_stack,
            rewind_state.store_data,
            rewind_result,
        );
        if res != Errno::Success {
            return Err(res);
        }
    }

    // Now invoke the module
    let ret = call_module_internal(&ctx, &mut store);

    // If it went to deep sleep then we need to handle that
    match ret {
        Ok(ret) => {
            // Frees the handle so that it closes
            drop(thread_handle);
            Ok(ret as Pid)
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
                        Some((rewind, RewindResultType::RewindWithResult(trigger_res))),
                    );
                }
            };

            /// Spawns the WASM process after a trigger
            unsafe {
                tasks.resume_wasm_after_poller(Box::new(respawn), ctx, store, deep.trigger)
            };
            Err(Errno::Unknown)
        }
    }
}
