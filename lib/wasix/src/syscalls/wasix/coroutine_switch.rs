use core::panic;
use std::sync::RwLock;
use super::*;
use crate::{run_wasi_func, run_wasi_func_start, syscalls::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoroutineState {
    Created,
    Active,
    Deleted,
    Failed,
}

struct SomeSortOfResumer {}

#[derive(Clone)]
pub struct CoroutineStack {
    /// The entrypoint function index
    pub entrypoint: u32,
    /// The current state of the coroutine
    pub state: CoroutineState,
    /// Some sort of gadget that allows us to resume the coroutine
    pub resumer: Option<u32>,
}

/// ### `coroutine_delete()`
#[instrument(level = "trace", skip_all, ret)]
pub fn coroutine_delete<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    coroutine: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    Ok(Errno::Success)
}

/// ### `coroutine_context()`
#[instrument(level = "trace", skip_all, ret)]
pub fn coroutine_context<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    entrypoint: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let root_coroutine = CoroutineStack {
        entrypoint,
        state: CoroutineState::Created,
        resumer: None,
    };

    let mut coroutines = env.coroutines.write().unwrap();
    coroutines.insert(0, Arc::new(RwLock::new(root_coroutine)));
}

    // let next_coroutine = env.next_coroutine.write().unwrap();
    // *next_coroutine = Some(0);
    let mut next_coroutine_id:u32 = 0;

    // TODO: Nested calls to coroutine_context are forbidden

    loop {
        let (env, mut store) = ctx.data_and_store_mut();
        // let memory = unsafe { env.memory_view(&ctx) };

        // TODO: Next can probably be a local
        // let mut next_coroutine = env.next_coroutine.write().unwrap();
        // let next_coroutine_id = (*next_coroutine).clone();
        // *next_coroutine = None;

        // let Some(next_coroutine_id) = next_coroutine_id else {
        //     panic!("No next coroutine to switch to");
        // };

        let coroutine = {
        let mut coroutines = env.coroutines.write().unwrap();

        let coroutine = coroutines.get_mut(&next_coroutine_id);
        let Some(coroutine) = coroutine else {
            panic!("Switching to invalid coroutine is not supported yet");
            return Err(WasiError::Exit(Errno::Inval.into()));
        };
        coroutine.clone()
        };

        let coroutine_state = coroutine.read().unwrap().state;
        match coroutine_state {
            CoroutineState::Created => {

                let function_id = coroutine.read().unwrap().entrypoint; // resolve function from index
                let function = 
                    env.inner()
                        .indirect_function_table_lookup(&mut store, function_id)
                        .expect("Function not found in table")
                        // .map_err(Errno::from)
                ;
                // Start the coroutine
                coroutine.write().unwrap().state = CoroutineState::Active;
                let resumer = function.call(&mut store, &[]); // TODO: Handle params
                let Err(err) = resumer else {
                    panic!("Coroutine entrypoint returned normally, which is not supported");
                };
                let Some((resumable, next)) = err.to_continuation() else {
                    panic!("Coroutine entrypoint did not return a continuation {:?}", err);
                    // return Err();
                };
                coroutine.write().unwrap().resumer = Some(resumable);
                
                // let mut next_coroutine = env.next_coroutine.write().unwrap();
                // if next_coroutine.is_some() {
                //     unreachable!("Next coroutine is set when we want to set it to another next");
                // }
                next_coroutine_id = next;

            }
            CoroutineState::Active => {
                let function_id = coroutine.read().unwrap().entrypoint; // resolve function from index
                let function =  
                    env.inner()
                        .indirect_function_table_lookup(&mut store, function_id)
                        // .map_err(Errno::from)
                        .expect("Function not found in table");
                // Start the coroutine
                let continuation = coroutine.read().unwrap().resumer.expect("Next coroutine has no resumer");
                let resumer = function.call_resume(&mut store, continuation); // TODO: Handle params
                let Err(err) = resumer else {
                    panic!("Coroutine returned normally, which is not supported");
                };
                let Some((resumable, next)) = err.to_continuation() else {
                    panic!("Coroutine did not return a continuation {:?}", err);
                    // return Err(err.into());
                };
                // // It may actually be possible to get a different continuation here, if the coroutine called a wasm function via a syscall
                // assert_eq!(continuation, resumable);
                coroutine.write().unwrap().resumer = Some(resumable);
                
                // let mut next_coroutine = env.next_coroutine.write().unwrap();
                // if next_coroutine.is_some() {
                //     unreachable!("Next coroutine is set when we want to set it to another next");
                // }
                // *next_coroutine = Some(next);
                next_coroutine_id = next;
            }
            CoroutineState::Deleted | CoroutineState::Failed => {
                panic!("Switching to deleted or failed coroutine is not supported");
                return Err(WasiError::Exit(Errno::Inval.into()));
            }
        }
    }

    // wasi_try_mem_ok!(new_coroutine_ptr.write(&memory, new_coroutine_id));
    unreachable!();

    Ok(Errno::Success)
}

/// ### `coroutine_new()`
#[instrument(level = "trace", skip_all, ret)]
pub fn coroutine_new<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_coroutine_ptr: WasmPtr<u32, M>,
    entrypoint: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let new_coroutine = CoroutineStack {
        entrypoint,
        state: CoroutineState::Created,
        resumer: None,
    };

    let new_coroutine_id = env
        .next_coroutine_id
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let mut coroutines = env.coroutines.write().unwrap();
    coroutines.insert(new_coroutine_id, Arc::new(RwLock::new(new_coroutine)));
    new_coroutine_ptr.write(&memory, new_coroutine_id).unwrap();

    Ok(Errno::Success)
}

/// ### `coroutine_switch()`
#[instrument(level = "trace", skip_all, ret)]
pub fn coroutine_switch<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    next_coroutine: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    // resumable will be filled in by the trap handler
    let trap = wasmer::sys::vm::Trap::Continuation { resumable: None, next: next_coroutine };
    unsafe {
        // Ideally this should suspend and cause couroutine_context to start switching
        wasmer::sys::vm::raise_lib_trap(trap);
    }


    // We expect to just continue here once we are resumed

    // if coroutine == 0 {
    //     panic!("Switching to coroutine 0 (main) is not supported yet");
    //     return Err(WasiError::Exit(Errno::Inval.into()));
    // }

    // let mut coroutines = env.coroutines.write().unwrap();
    // let coroutine = coroutines.get_mut(&coroutine);
    // let Some(coroutine) = coroutine else {
    //     panic!("Switching to invalid coroutine is not supported yet");
    //     return Err(WasiError::Exit(Errno::Inval.into()));
    // };
    // if matches!(
    //     coroutine.state,
    //     CoroutineState::Deleted | CoroutineState::Failed
    // ) {
    //     panic!("Switching to deleted or failed coroutine is not supported");
    //     return Err(WasiError::Exit(Errno::Inval.into()));
    // }

    // let first_start = matches!(coroutine.state, CoroutineState::Created);

    // 1. Indicate what's the next coroutine to run
    // 2. suspend yourself

    // if first_start {
    // run_wasi_func(func, store, params)
    // run_wasi_func_start(func, store)

    //     let function = coroutine.entrypoint; // resolve function from index
    //     let own_resumer = function.run_resumable();
    //     let own_coroutine.resumer = Some(own_resumer);
    // } else {

    //     // resume coroutine
    //     let Some(resumer) = &coroutine.resumer else {
    //         panic!("Coroutine has no resumer");
    //     };
    //     resumer.resume();
    // }

    Ok(Errno::Success)
}
