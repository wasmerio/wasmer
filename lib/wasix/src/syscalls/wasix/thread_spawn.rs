use std::f32::consts::E;

use super::*;
#[cfg(feature = "journal")]
use crate::journal::JournalEffector;
use crate::{
    WasiThreadHandle,
    os::task::thread::{WasiMemoryLayout, context_switching::ContextSwitchingContext},
    runtime::{
        TaintReason,
        task_manager::{TaskWasm, TaskWasmRunProperties},
    },
    syscalls::*,
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
#[instrument(level = "trace", skip_all, ret)]
pub fn thread_spawn_v2<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
    ret_tid: WasmPtr<Tid, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    // Create the thread
    let tid = wasi_try_ok!(thread_spawn_internal_from_wasi(&mut ctx, start_ptr));

    // Success
    let memory = unsafe { ctx.data().memory_view(&ctx) };
    wasi_try_mem_ok!(ret_tid.write(&memory, tid));

    tracing::debug!(
        tid,
        from_tid = ctx.data().thread.id().raw(),
        "spawned new thread"
    );

    Ok(Errno::Success)
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
        let stack_upper: u64 = start.stack_upper.into();
        let stack_size: u64 = start.stack_size.into();
        let guard_size: u64 = start.guard_size.into();
        let tls_base: u64 = start.tls_base.into();
        let stack_lower = stack_upper - stack_size;

        WasiMemoryLayout {
            stack_upper,
            stack_lower,
            guard_size,
            stack_size,
            tls_base: Some(tls_base),
        }
    };
    tracing::trace!(
        from_tid = env.thread.id().raw(),
        "thread_spawn with layout {:?}",
        layout
    );

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
    let func_env = ctx.as_ref();
    let mut store = ctx.as_store_mut();
    let env = func_env.as_ref(&store);
    let tasks = env.tasks().clone();

    let env_inner = env.inner();
    let module_handles = env_inner.main_module_instance_handles();

    let thread_memory = module_handles.memory_clone();
    let linker = env_inner.linker().cloned();

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
    if module_handles.thread_spawn.is_none() {
        warn!("thread failed - the program does not export a `wasi_thread_start` function");
        return Err(Errno::Notcapable);
    }
    let thread_module = module_handles.module_clone();
    let spawn_type = match linker {
        Some(linker) => crate::runtime::SpawnType::NewLinkerInstanceGroup(linker, func_env, store),
        None => crate::runtime::SpawnType::ShareMemory(thread_memory, store.as_store_ref()),
    };

    // Now spawn a thread
    trace!("threading: spawning background thread");
    let run = move |props: TaskWasmRunProperties| {
        execute_module(props.ctx, props.store);
    };

    let mut task_wasm = TaskWasm::new(Box::new(run), thread_env, thread_module, false, false)
        .with_memory(spawn_type);

    tasks.task_wasm(task_wasm).map_err(Into::<Errno>::into)?;

    // Success
    Ok(())
}

// This function calls into the module
fn call_module_internal<M: MemorySize>(
    ctx: &WasiFunctionEnv,
    store: &mut Store,
    start_ptr_offset: M::Offset,
) -> Result<(), DeepSleepWork> {
    // We either call the reactor callback or the thread spawn callback
    //trace!("threading: invoking thread callback (reactor={})", reactor);

    // Note: we ensure both unwraps can happen before getting to this point
    let spawn = ctx
        .data(&store)
        .inner()
        .main_module_instance_handles()
        .thread_spawn
        .clone()
        .unwrap();
    let tid = ctx.data(&store).tid();
    // TODO: Find a better way to get a Function from a TypedFunction
    // SAFETY: no
    let spawn = unsafe { std::mem::transmute::<TypedFunction<(i32, i32), ()>, Function>(spawn) };
    let tid_i32 = tid.raw().try_into().map_err(|_| Errno::Overflow).unwrap();
    let start_pointer_i32 = start_ptr_offset
        .try_into()
        .map_err(|_| Errno::Overflow)
        .unwrap();
    let thread_result = ContextSwitchingContext::run_main_context(
        ctx,
        store,
        spawn,
        vec![Value::I32(tid_i32), Value::I32(start_pointer_i32)],
    )
    .map(|_| ());

    trace!("callback finished (ret={:?})", thread_result);

    let exit_code = handle_thread_result(ctx, store, thread_result)?;

    // Clean up the environment on exit
    ctx.on_exit(store, exit_code);
    Ok(())
}

fn handle_thread_result(
    env: &WasiFunctionEnv,
    store: &mut Store,
    err: Result<(), RuntimeError>,
) -> Result<Option<ExitCode>, DeepSleepWork> {
    let tid = env.data(&store).tid();
    let pid = env.data(&store).pid();
    let Err(err) = err else {
        trace!("thread exited cleanly without calling thread_exit");
        return Ok(None);
    };
    match err.downcast::<WasiError>() {
        Ok(WasiError::ThreadExit) => {
            trace!("thread exited cleanly");
            Ok(None)
        }
        Ok(WasiError::Exit(code)) => {
            trace!(exit_code = ?code, "thread requested exit");
            if !code.is_success() {
                // TODO: Why do we need to taint the runtime on a non-zero exit code? Why not also for zero?
                env.data(&store)
                    .runtime
                    .on_taint(TaintReason::NonZeroExitCode(code));
            };
            Ok(Some(code))
        }
        Ok(WasiError::DeepSleep(deep)) => {
            trace!("entered a deep sleep");
            Err(deep)
        }
        Ok(WasiError::UnknownWasiVersion) => {
            eprintln!(
                "Thread {tid} of process {pid} failed because it has an unknown wasix version"
            );
            env.data(&store)
                .runtime
                .on_taint(TaintReason::UnknownWasiVersion);
            Ok(Some(ExitCode::from(129)))
        }
        Ok(WasiError::DlSymbolResolutionFailed(symbol)) => {
            eprintln!("Thread {tid} of process {pid} failed to find required symbol: {symbol}");
            env.data(&store)
                .runtime
                .on_taint(TaintReason::DlSymbolResolutionFailed(symbol.clone()));
            Ok(Some(ExitCode::from(129)))
        }
        Err(err) => {
            eprintln!("Thread {tid} of process {pid} failed with runtime error: {err}");
            env.data(&store)
                .runtime
                .on_taint(TaintReason::RuntimeError(err));
            Ok(Some(ExitCode::from(129)))
        }
    }
}

/// Calls the module
fn call_module<M: MemorySize>(
    mut ctx: WasiFunctionEnv,
    mut store: Store,
    start_ptr_offset: M::Offset,
    thread_handle: Arc<WasiThreadHandle>,
    rewind_state: Option<(RewindState, RewindResultType)>,
) {
    let env = ctx.data(&store);
    let tasks = env.tasks().clone();

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
            return;
        }
    }

    // Now invoke the module
    let ret = call_module_internal::<M>(&ctx, &mut store, start_ptr_offset);

    // If it went to deep sleep then we need to handle that
    if let Err(deep) = ret {
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
        return;
    };
    // I don't think we need to do this explicitly, but it was done before refactoring so we keep it for now.
    drop(thread_handle);
}
