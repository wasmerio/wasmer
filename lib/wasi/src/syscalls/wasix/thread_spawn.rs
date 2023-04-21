use super::*;
use crate::syscalls::*;

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
#[instrument(level = "debug", skip_all, ret)]
pub fn thread_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    // Create the thread
    let tid = wasi_try!(thread_spawn_internal(&ctx, start_ptr));

    // Success
    let memory = ctx.data().memory_view(&ctx);
    wasi_try_mem!(ret_tid.write(&memory, tid));
    Errno::Success
}

pub(crate) fn thread_spawn_internal<M: MemorySize>(
    ctx: &FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
) -> Result<Tid, Errno> {
    // Now we use the environment and memory references
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let runtime = env.runtime.clone();
    let tasks = env.tasks().clone();

    // Read the properties about the stack which we will use for asyncify
    let start = start_ptr.read(&memory).map_err(mem_error_to_wasi)?;
    let stack_start: u64 = start.stack_start.try_into().map_err(|_| Errno::Overflow)?;
    let stack_size: u64 = start.stack_size.try_into().map_err(|_| Errno::Overflow)?;
    let stack_base = stack_start - stack_size;

    // Create the handle that represents this thread
    let mut thread_handle = match env.process.new_thread() {
        Ok(h) => h,
        Err(err) => {
            error!(
                %stack_base,
                caller_id = current_caller_id().raw(),
                "failed to create thread handle",
            );
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Err(Errno::Access);
        }
    };
    let thread_id: Tid = thread_handle.id().into();
    Span::current().record("tid", thread_id);

    let mut store = ctx.data().runtime.new_store();

    // This function takes in memory and a store and creates a context that
    // can be used to call back into the process
    let create_ctx = {
        let state = env.state.clone();
        let wasi_env = env.duplicate();
        let thread = thread_handle.as_thread();
        move |mut store: Store, module: Module, memory: Memory| {
            // We need to reconstruct some things
            let module = module;
            // Build the context object and import the memory
            let mut ctx = WasiFunctionEnv::new(&mut store, wasi_env.duplicate());
            {
                let env = ctx.data_mut(&mut store);
                env.thread = thread.clone();
                env.stack_end = stack_base;
                env.stack_start = stack_start;
            }

            let (mut import_object, init) =
                import_object_for_all_wasi_versions(&module, &mut store, &ctx.env);
            import_object.define("env", "memory", memory.clone());

            let instance = match Instance::new(&mut store, &module, &import_object) {
                Ok(a) => a,
                Err(err) => {
                    error!("failed - create instance failed: {}", err);
                    return Err(Errno::Noexec as u32);
                }
            };

            init(&instance, &store).unwrap();

            // Set the current thread ID
            ctx.data_mut(&mut store).inner =
                Some(WasiInstanceHandles::new(memory, &store, instance));
            Ok(WasiThreadContext {
                ctx,
                store: RefCell::new(store),
            })
        }
    };

    // We need a copy of the process memory and a packaged store in order to
    // launch threads and reactors
    let thread_memory = ctx
        .data()
        .memory()
        .clone_in_store(&ctx, &mut store)
        .ok_or_else(|| {
            error!("failed - the memory could not be cloned");
            Errno::Notcapable
        })?;

    // This function calls into the module
    let start_ptr_offset = start_ptr.offset();
    let call_module = move |ctx: &WasiFunctionEnv, store: &mut Store| {
        // We either call the reactor callback or the thread spawn callback
        //trace!("threading: invoking thread callback (reactor={})", reactor);
        let spawn = ctx.data(&store).inner().thread_spawn.clone().unwrap();
        let tid = ctx.data(&store).tid();
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
        ctx.cleanup(store, Some(ret.into()));

        // Return the result
        ret as u32
    };

    // This next function gets a context for the local thread and then
    // calls into the process
    let mut execute_module = {
        let state = env.state.clone();
        move |store: &mut Option<Store>, module: Module, memory: &mut Option<Memory>| {
            // We capture the thread handle here, it is used to notify
            // anyone that is interested when this thread has terminated
            let _captured_handle = Box::new(&mut thread_handle);

            // Given that it is not safe to assume this delegate will run on the
            // same thread we need to capture a simple process that will create
            // context objects on demand and reuse them
            let caller_id = current_caller_id();

            // We loop because read locks are held while functions run which need
            // to be relocked in the case of a miss hit.
            loop {
                let thread = {
                    let guard = state.threading.read().unwrap();
                    guard.thread_ctx.get(&caller_id).cloned()
                };
                if let Some(thread) = thread {
                    let mut store = thread.store.borrow_mut();
                    let ret = call_module(&thread.ctx, store.deref_mut());

                    {
                        let mut guard = state.threading.write().unwrap();
                        guard.thread_ctx.remove(&caller_id);
                    }

                    return ret;
                }

                // Otherwise we need to create a new context under a write lock
                debug!(
                    "encountered a new caller (ref={}) - creating WASM execution context...",
                    caller_id.raw()
                );

                // We can only create the context once per thread
                let memory = match memory.take() {
                    Some(m) => m,
                    None => {
                        debug!("failed - memory can only be consumed once per context creation");
                        return Errno::Noexec as u32;
                    }
                };
                let store = match store.take() {
                    Some(s) => s,
                    None => {
                        debug!("failed - store can only be consumed once per context creation");
                        return Errno::Noexec as u32;
                    }
                };

                // Now create the context and hook it up
                let mut guard = state.threading.write().unwrap();
                let ctx = match create_ctx(store, module.clone(), memory) {
                    Ok(c) => c,
                    Err(err) => {
                        return err;
                    }
                };
                guard.thread_ctx.insert(caller_id, Arc::new(ctx));
            }
        }
    };

    // If the process does not export a thread spawn function then obviously
    // we can't spawn a background thread
    if env.inner().thread_spawn.is_none() {
        warn!("thread failed - the program does not export a `wasi_thread_start` function");
        return Err(Errno::Notcapable);
    }
    let spawn_type = crate::runtime::SpawnType::NewThread(thread_memory);

    // Now spawn a thread
    trace!("threading: spawning background thread");
    let thread_module = env.inner().instance.module().clone();
    let tasks2 = tasks.clone();

    let task = move |store, thread_module, mut thread_memory| {
        // FIXME: should not use unwrap() here! (initializiation refactor)
        let mut store = Some(store);
        execute_module(&mut store, thread_module, &mut thread_memory);
    };

    tasks
        .task_wasm(Box::new(task), store, thread_module, spawn_type)
        .map_err(Into::<Errno>::into)?;

    // Success
    Ok(thread_id)
}
