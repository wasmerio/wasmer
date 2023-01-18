use super::*;
use crate::syscalls::*;

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
pub fn thread_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    user_data: u64,
    stack_base: u64,
    stack_start: u64,
    reactor: Bool,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::thread_spawn (reactor={:?}, thread_id={}, stack_base={}, caller_id={})",
        ctx.data().pid(),
        ctx.data().tid(),
        reactor,
        ctx.data().thread.tid().raw(),
        stack_base,
        current_caller_id().raw()
    );

    // Now we use the environment and memory references
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let runtime = env.runtime.clone();
    let tasks = env.tasks.clone();

    // Create the handle that represents this thread
    let mut thread_handle = match env.process.new_thread() {
        Ok(h) => h,
        Err(err) => {
            error!(
                "wasi[{}:{}]::thread_spawn (reactor={:?}, thread_id={}, stack_base={}, caller_id={}) - failed to create thread handle: {}",
                ctx.data().pid(),
                ctx.data().tid(),
                reactor,
                ctx.data().thread.tid().raw(),
                stack_base,
                current_caller_id().raw(),
                err
            );
            // TODO: evaluate the appropriate error code, document it in the spec.
            return Errno::Access;
        }
    };
    let thread_id: Tid = thread_handle.id().into();

    // We need a copy of the process memory and a packaged store in order to
    // launch threads and reactors
    let thread_memory = wasi_try!(ctx.data().memory().try_clone(&ctx).ok_or_else(|| {
        error!("thread failed - the memory could not be cloned");
        Errno::Notcapable
    }));

    let mut store = ctx.data().runtime.new_store();

    // This function takes in memory and a store and creates a context that
    // can be used to call back into the process
    let create_ctx = {
        let state = env.state.clone();
        let wasi_env = env.clone();
        let thread = thread_handle.as_thread();
        move |mut store: Store, module: Module, memory: VMMemory| {
            // We need to reconstruct some things
            let module = module.clone();
            let memory = Memory::new_from_existing(&mut store, memory);

            // Build the context object and import the memory
            let mut ctx = WasiFunctionEnv::new(&mut store, wasi_env.clone());
            {
                let env = ctx.data_mut(&mut store);
                env.thread = thread.clone();
                env.stack_base = stack_base;
                env.stack_start = stack_start;
            }

            let (mut import_object, init) =
                import_object_for_all_wasi_versions(&module, &mut store, &ctx.env);
            import_object.define("env", "memory", memory.clone());

            let instance = match Instance::new(&mut store, &module, &import_object) {
                Ok(a) => a,
                Err(err) => {
                    error!("thread failed - create instance failed: {}", err);
                    return Err(Errno::Noexec as u32);
                }
            };

            init(&instance, &store).unwrap();

            // Set the current thread ID
            ctx.data_mut(&mut store).inner =
                Some(WasiEnvInner::new(module, memory, &store, &instance));
            trace!(
                "threading: new context created for thread_id = {}",
                thread.tid().raw()
            );
            Ok(WasiThreadContext {
                ctx,
                store: RefCell::new(store),
            })
        }
    };

    // This function calls into the module
    let call_module = move |ctx: &WasiFunctionEnv, store: &mut Store| {
        // We either call the reactor callback or the thread spawn callback
        //trace!("threading: invoking thread callback (reactor={})", reactor);
        let spawn = match reactor {
            Bool::False => ctx.data(&store).inner().thread_spawn.clone().unwrap(),
            Bool::True => ctx.data(&store).inner().react.clone().unwrap(),
            _ => {
                debug!("thread failed - failed as the reactor type is not value");
                return Errno::Noexec as u32;
            }
        };

        let user_data_low: u32 = (user_data & 0xFFFFFFFF) as u32;
        let user_data_high: u32 = (user_data >> 32) as u32;

        let mut ret = Errno::Success;
        if let Err(err) = spawn.call(store, user_data_low as i32, user_data_high as i32) {
            debug!("thread failed - start: {}", err);
            ret = Errno::Noexec;
        }
        //trace!("threading: thread callback finished (reactor={}, ret={})", reactor, ret);

        // If we are NOT a reactor then we will only run once and need to clean up
        if reactor == Bool::False {
            // Clean up the environment
            ctx.cleanup(store, Some(ret as ExitCode));
        }

        // Return the result
        ret as u32
    };

    // This next function gets a context for the local thread and then
    // calls into the process
    let mut execute_module = {
        let state = env.state.clone();
        move |store: &mut Option<Store>, module: Module, memory: &mut Option<VMMemory>| {
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
                    guard.thread_ctx.get(&caller_id).map(|a| a.clone())
                };
                if let Some(thread) = thread {
                    let mut store = thread.store.borrow_mut();
                    let ret = call_module(&thread.ctx, store.deref_mut());
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
                        debug!(
                            "thread failed - memory can only be consumed once per context creation"
                        );
                        return Errno::Noexec as u32;
                    }
                };
                let store = match store.take() {
                    Some(s) => s,
                    None => {
                        debug!(
                            "thread failed - store can only be consumed once per context creation"
                        );
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

    // If we are a reactor then instead of launching the thread now
    // we store it in the state machine and only launch it whenever
    // work arrives that needs to be processed
    match reactor {
        Bool::True => {
            warn!("thread failed - reactors are not currently supported");
            return Errno::Notcapable;
        }
        Bool::False => {
            // If the process does not export a thread spawn function then obviously
            // we can't spawn a background thread
            if env.inner().thread_spawn.is_none() {
                warn!("thread failed - the program does not export a _start_thread function");
                return Errno::Notcapable;
            }

            // Now spawn a thread
            trace!("threading: spawning background thread");
            let thread_module = env.inner().module.clone();
            wasi_try!(tasks
                .task_wasm(
                    Box::new(move |store, module, thread_memory| {
                        let mut thread_memory = thread_memory;
                        let mut store = Some(store);
                        execute_module(&mut store, module, &mut thread_memory);
                    }),
                    store,
                    thread_module,
                    crate::runtime::SpawnType::NewThread(thread_memory)
                )
                .map_err(|err| { Into::<Errno>::into(err) }));
        }
        _ => {
            warn!("thread failed - invalid reactor parameter value");
            return Errno::Notcapable;
        }
    }

    // Success
    let memory = ctx.data().memory_view(&ctx);
    wasi_try_mem!(ret_tid.write(&memory, thread_id));
    Errno::Success
}
