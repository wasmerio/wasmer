use super::*;
#[cfg(feature = "journal")]
use crate::journal::JournalEffector;
use crate::{
    WasiThreadHandle,
    os::task::thread::WasiMemoryLayout,
    runtime::{
        TaintReason,
        task_manager::{TaskWasm, TaskWasmRunProperties},
    },
    state::context_switching::ContextSwitchingEnvironment,
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
        let stack_lower = stack_upper.checked_sub(stack_size).ok_or(Errno::Inval)?;

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

/// Largest thread ID that may be handed to `wasi_thread_start`.
///
/// The wasi-threads spec restricts TIDs to `[1, 2^29)`: the sign bit is used
/// by `thread-spawn` to report errors, and libc implementations reserve the
/// next bits in their lock words (musl stores the owner TID in the low 30
/// bits and uses `0x3fffffff` as a sentinel).
const MAX_THREAD_ID: u32 = (1 << 29) - 1;

/// Arguments for the guest's `wasi_thread_start(tid: i32, start_arg: i32)`.
///
/// They are validated when the thread is spawned so that a bad value is
/// reported to the caller as an errno instead of failing in the new thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThreadStartArgs {
    tid: i32,
    start_arg: i32,
}

impl ThreadStartArgs {
    fn new(tid: WasiThreadId, start_ptr: u64) -> Result<Self, Errno> {
        let tid = tid.raw();
        if !(1..=MAX_THREAD_ID).contains(&tid) {
            tracing::warn!(
                tid,
                "thread ID is outside the range allowed by wasi-threads"
            );
            return Err(Errno::Again);
        }
        let start_arg = wasm32_ptr_to_i32_arg(start_ptr).inspect_err(|_| {
            tracing::warn!(
                start_ptr,
                "thread start pointer does not fit into the i32 argument of wasi_thread_start"
            );
        })?;

        Ok(Self {
            tid: tid.cast_signed(),
            start_arg,
        })
    }

    fn to_values(self) -> Vec<Value> {
        vec![Value::I32(self.tid), Value::I32(self.start_arg)]
    }
}

pub fn thread_spawn_internal_using_layout<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    thread_handle: Arc<WasiThreadHandle>,
    layout: WasiMemoryLayout,
    start_ptr_offset: M::Offset,
    rewind_state: Option<(RewindState, RewindResultType)>,
) -> Result<(), Errno> {
    let start_args = ThreadStartArgs::new(thread_handle.id(), start_ptr_offset.into())?;

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
            call_module::<M>(ctx, store, start_args, thread_handle, rewind_state)
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
        Some(linker) => {
            let instance_group_data = linker.prepare_for_instance_group(ctx).map_err(|e| {
                tracing::warn!("failed to prepare linker for thread spawn: {e}");
                Errno::Notcapable
            })?;
            crate::runtime::SpawnType::NewLinkerInstanceGroup(instance_group_data)
        }
        None => crate::runtime::SpawnType::AttachMemory(
            thread_memory.as_shared(&store).ok_or_else(|| {
                tracing::warn!("Memory must be shared for thread spawning to work");
                Errno::Memviolation
            })?,
        ),
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
fn call_module_internal(
    ctx: &WasiFunctionEnv,
    mut store: Store,
    start_args: ThreadStartArgs,
) -> (Store, Result<Option<ExitCode>, DeepSleepWork>) {
    // The spawning thread checked that the module exports `wasi_thread_start`,
    // and this thread runs an instance of the same module.
    let Some(spawn) = ctx
        .data(&store)
        .inner()
        .main_module_instance_handles()
        .thread_spawn
        .clone()
    else {
        error!("thread failed - the program does not export a `wasi_thread_start` function");
        return (store, Ok(Some(Errno::Notcapable.into())));
    };

    let spawn: Function = spawn.into();
    let (mut store, thread_result) =
        ContextSwitchingEnvironment::run_main_context(ctx, store, spawn, start_args.to_values());
    let thread_result = thread_result.map(|_| ());

    trace!("callback finished (ret={:?})", thread_result);

    let exit_code = match handle_thread_result(ctx, &mut store, thread_result) {
        Ok(code) => code,
        Err(deep_sleep) => return (store, Err(deep_sleep)),
    };

    (store, Ok(exit_code))
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
            if err.clone().to_trap() == Some(wasmer_types::TrapCode::HostInterrupt) {
                debug!(%tid, %pid, error = %err, "thread interrupted by host");
            } else {
                eprintln!("Thread {tid} of process {pid} failed with runtime error: {err}");
            }
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
    start_args: ThreadStartArgs,
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
    let (mut store, ret) = call_module_internal(&ctx, store, start_args);

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
                    start_args,
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

    let exit_code = ret.unwrap_or_else(|_| unreachable!());
    if let Some(exit_code) = exit_code {
        ctx.on_exit(&mut store, Some(exit_code));
        thread_handle.set_status_finished(Ok(exit_code));
    } else {
        ctx.on_exit(&mut store, None);
        thread_handle.set_status_finished(Ok(Errno::Success.into()));
    }

    drop(thread_handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    const TID: u32 = 2;

    fn start_args(tid: u32, start_ptr: u64) -> Result<ThreadStartArgs, Errno> {
        ThreadStartArgs::new(WasiThreadId::from(tid), start_ptr)
    }

    #[test]
    fn start_args_keep_bits_of_pointers_above_2gib() {
        let args = start_args(TID, 0x8000_0000).unwrap();
        assert_eq!(args.start_arg, i32::MIN);
        assert_eq!(args.start_arg.cast_unsigned(), 0x8000_0000);

        let args = start_args(TID, u32::MAX.into()).unwrap();
        assert_eq!(args.start_arg, -1);

        let args = start_args(TID, 0x7fff_fff0).unwrap();
        assert_eq!(args.start_arg, 0x7fff_fff0);
    }

    #[test]
    fn start_args_reject_pointers_wider_than_32_bits() {
        assert_eq!(start_args(TID, 1 << 32), Err(Errno::Overflow));
        assert_eq!(start_args(TID, u64::MAX), Err(Errno::Overflow));
    }

    #[test]
    fn start_args_enforce_wasi_threads_tid_range() {
        let args = start_args(MAX_THREAD_ID, 0).unwrap();
        assert_eq!(args.tid, 0x1fff_ffff);

        for tid in [0, MAX_THREAD_ID + 1, i32::MAX.cast_unsigned(), u32::MAX] {
            assert_eq!(
                start_args(tid, 0),
                Err(Errno::Again),
                "tid {tid:#x} must be rejected"
            );
        }
    }
}
