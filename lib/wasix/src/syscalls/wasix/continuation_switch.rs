use wasmer::{Continuation, StoreMut, Tag, Type};

use super::*;
use crate::{run_wasi_func, run_wasi_func_start, syscalls::*};
use core::panic;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoroutineState {
    Created,
    Active,
    Deleted,
    Failed,
}

const MAIN_CONTINUATION_ID: u32 = 0;

struct SomeSortOfResumer {}

#[derive(Clone)]
pub struct CoroutineStack {
    /// The entrypoint function index
    pub entrypoint: Option<u32>,
    /// The current state of the coroutine
    pub state: CoroutineState,
    pub parent: Option<u32>,
    /// Some sort of gadget that allows us to resume the coroutine
    pub resumer: Option<Continuation>,
}

/// ### `coroutine_delete()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn continuation_delete(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    coroutine: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory: MemoryView<'_> = unsafe { env.memory_view(&ctx) };

    Ok(Errno::Success)
}

thread_local! {
    static CURRENT_COROUTINE_ID: std::cell::RefCell<u32> = std::cell::RefCell::new(MAIN_CONTINUATION_ID);
}

/// ### `coroutine_context()`
#[instrument(level = "trace", skip(env, store), ret)]
pub fn call_in_context(
    // ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    env: &WasiEnv,
    mut store: StoreMut<'_>,
    entrypoint: &wasmer::Function,
    params: &[wasmer::Value],
) -> Result<Box<[Value]>, RuntimeError> {
    let main_continuation_id = {
        // let env = ctx.data();
        // let memory = unsafe { env.memory_view(&ctx) };

        let root_coroutine = CoroutineStack {
            entrypoint: None,
            state: CoroutineState::Created,
            resumer: None,
            parent: None,
        };

        let mut coroutines = env.coroutines.write().unwrap();

        let new_coroutine_id = env
            .next_coroutine_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        coroutines.insert(new_coroutine_id, Arc::new(RwLock::new(root_coroutine)));
        new_coroutine_id
    };

    let mut parent_id = None;
    let mut next_continuation_id = main_continuation_id;

    // let next_coroutine = env.next_coroutine.write().unwrap();
    // *next_coroutine = Some(0);

    // TODO: Nested calls to coroutine_context are forbidden

    loop {
        // let (env, mut store) = ctx.data_and_store_mut();
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

            let coroutine = coroutines.get_mut(&next_continuation_id);
            let Some(coroutine) = coroutine else {
                panic!("Switching to invalid coroutine is not supported yet");
                // return Err(WasiError::Exit(Errno::Inval.into()));
            };
            coroutine.clone()
        };
        let is_context_main = next_continuation_id == main_continuation_id;

        let coroutine_state = coroutine.read().unwrap().state;
        match coroutine_state {
            CoroutineState::Created => {
                let function_id = coroutine.read().unwrap().entrypoint; // resolve function from index
                let function = function_id.map(|function_id| {
                    env.inner()
                        .indirect_function_table_lookup(&mut store, function_id)
                        .expect("Function not found in table")
                });
                let function = function.as_ref().unwrap_or(entrypoint);
                // .map_err(Errno::from)
                // Start the coroutine
                
                coroutine.write().unwrap().state = CoroutineState::Active;
                coroutine.write().unwrap().parent = parent_id;
                parent_id = Some(next_continuation_id as u32);
                let resumer = function.call(&mut store, &[]); // TODO: Handle params
                // println!("Coroutine started, got resumero {:?}", resumer);
                // eprintln!("Coroutine started, got resumere {:?}", resumer);
                let err = match resumer {
                    Ok(result) => {
                        let parent = coroutine.read().unwrap().parent;
                        let Some(parent) = parent else {
                            // Main function returned
                            return Ok(result);
                        };
                        parent_id = Some(parent);
                        next_continuation_id = parent as u64;
                        continue;
                        
                        todo!();
                        // get parent
                        // panic!("Coroutine entrypoint returned normally, which is not supported");
                    }
                    Err(err) => err,
                };
                let Some(continuation) = err.to_continuation() else {
                    // if is_context_main {
                    // TODO: This is stupid
                    return Err(err);
                    // }
                    // panic!(
                    //     "Coroutine entrypoint did not return a continuation {:?}",
                    //     err
                    // );
                };
                let payload = continuation.payload(&mut store.as_store_mut());
                let Some(first) = payload.first() else {
                    panic!("Continuation has no payload");
                };
                let next = first.unwrap_i64() as u64;
                coroutine.write().unwrap().resumer = Some(continuation);
                
                // let mut next_coroutine = env.next_coroutine.write().unwrap();
                // if next_coroutine.is_some() {
                //     unreachable!("Next coroutine is set when we want to set it to another next");
                // }
                next_continuation_id = next;
            }
            CoroutineState::Active => {
                let function_id = coroutine.read().unwrap().entrypoint; // resolve function from index
                let function = function_id.map(|function_id| {
                    env.inner()
                        .indirect_function_table_lookup(&mut store, function_id)
                        .expect("Function not found in table")
                });
                let function = function.as_ref().unwrap_or(entrypoint);

                // Start the coroutine
                let continuation = coroutine
                    .read()
                    .unwrap()
                    .resumer.clone()
                    .expect("Next coroutine has no resumer");
                let resumer = function.call_resume(&mut store, continuation); // TODO: Handle params

                let err = match resumer {
                    Ok(result) => {
                        let parent = coroutine.read().unwrap().parent;
                        let Some(parent) = parent else {
                            // Main function returned
                            return Ok(result);
                        };
                        parent_id = Some(parent);
                        next_continuation_id = parent as u64;
                        continue;
                    }
                    Err(err) => err,
                };
                let Some(continuation) = err.to_continuation() else {
                    // if is_context_main {
                    // TODO: This is stupid
                    return Err(err);
                    // }
                    // panic!(
                    //     "Coroutine entrypoint did not return a continuation {:?}",
                    //     err
                    // );
                };

                // let Err(err) = resumer else {
                //     panic!("Coroutine returned normally, which is not supported");
                // };
                // let Some((resumable, next)) = err.to_continuation() else {
                //     panic!("Coroutine did not return a continuation {:?}", err);
                //     // return Err(err.into());
                // };
                // // It may actually be possible to get a different continuation here, if the coroutine called a wasm function via a syscall
                // assert_eq!(continuation, resumable);
                let payload = continuation.payload(&mut store.as_store_mut());
                let Some(first) = payload.first() else {
                    panic!("Continuation has no payload");
                };
                let next = first.unwrap_i64() as u64;
                coroutine.write().unwrap().resumer = Some(continuation);


                // let mut next_coroutine = env.next_coroutine.write().unwrap();
                // if next_coroutine.is_some() {
                //     unreachable!("Next coroutine is set when we want to set it to another next");
                // }
                // *next_coroutine = Some(next);
                next_continuation_id = next;
            }
            CoroutineState::Deleted | CoroutineState::Failed => {
                panic!("Switching to deleted or failed coroutine is not supported");
                // return Err(WasiError::Exit(Errno::Inval.into()));
            }
        }
    }

    // wasi_try_mem_ok!(new_coroutine_ptr.write(&memory, new_coroutine_id));
    unreachable!();

}

/// ### `coroutine_context()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn continuation_context<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_coroutine_ptr: WasmPtr<u32, M>,
    entrypoint: u32,
) -> Result<Errno, RuntimeError> {
    panic!("continuation_context is deprecated, use call_in_context instead");
//     match WasiEnv::do_pending_operations(&mut ctx) {
//         Ok(()) => {}
//         Err(e) => return Err(RuntimeError::user(Box::new(e))),
//     }

//     let main_continuation_id = {
//         let env = ctx.data();
//         let memory = unsafe { env.memory_view(&ctx) };

//         let root_coroutine = CoroutineStack {
//             entrypoint: Some(entrypoint),
//             state: CoroutineState::Created,
//             resumer: None,
//         };

//         let mut coroutines = env.coroutines.write().unwrap();

//         let new_coroutine_id = env
//             .next_coroutine_id
//             .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
//         coroutines.insert(new_coroutine_id, Arc::new(RwLock::new(root_coroutine)));
//         new_coroutine_ptr
//             .write(&memory, new_coroutine_id as u32)
//             .unwrap();
//         new_coroutine_id
//     };

//     let mut next_continuation_id = main_continuation_id;

//     // let next_coroutine = env.next_coroutine.write().unwrap();
//     // *next_coroutine = Some(0);

//     // TODO: Nested calls to coroutine_context are forbidden

//     loop {
//         let (env, mut store) = ctx.data_and_store_mut();
//         // let memory = unsafe { env.memory_view(&ctx) };

//         // TODO: Next can probably be a local
//         // let mut next_coroutine = env.next_coroutine.write().unwrap();
//         // let next_coroutine_id = (*next_coroutine).clone();
//         // *next_coroutine = None;

//         // let Some(next_coroutine_id) = next_coroutine_id else {
//         //     panic!("No next coroutine to switch to");
//         // };

//         let coroutine = {
//             let mut coroutines = env.coroutines.write().unwrap();

//             let coroutine = coroutines.get_mut(&next_continuation_id);
//             let Some(coroutine) = coroutine else {
//                 panic!("Switching to invalid coroutine is not supported yet");
//                 // return Err(WasiError::Exit(Errno::Inval.into()));
//             };
//             coroutine.clone()
//         };
//         let is_context_main = next_continuation_id == main_continuation_id;

//         let coroutine_state = coroutine.read().unwrap().state;
//         match coroutine_state {
//             CoroutineState::Created => {
//                 let function_id = coroutine.read().unwrap().entrypoint; // resolve function from index
//                 let function =
//                     env.inner()
//                         .indirect_function_table_lookup(&mut store, function_id.unwrap())
//                         .expect("Function not found in table")
//                         // .map_err(Errno::from)
//                 ;
//                 // Start the coroutine
//                 coroutine.write().unwrap().state = CoroutineState::Active;
//                 let resumer = function.call(&mut store, &[]); // TODO: Handle params
//                 // println!("Coroutine started, got resumero {:?}", resumer);
//                 // eprintln!("Coroutine started, got resumere {:?}", resumer);
//                 let err = match resumer {
//                     Ok(result) => {
//                         if is_context_main {
//                             // TODO: This is stupid
//                             return Ok(Errno::Success);
//                         }
//                         panic!("Coroutine entrypoint returned normally, which is not supported");
//                     }
//                     Err(err) => err,
//                 };
//                 let Some((continuation_ref, next)) = err.to_continuation_ref() else {
//                     if is_context_main {
//                         // TODO: This is stupid
//                         return Err(err);
//                     }
//                     panic!(
//                         "Coroutine entrypoint did not return a continuation {:?}",
//                         err
//                     );
//                 };
//                 coroutine.write().unwrap().resumer = Some(continuation_ref);

//                 // let mut next_coroutine = env.next_coroutine.write().unwrap();
//                 // if next_coroutine.is_some() {
//                 //     unreachable!("Next coroutine is set when we want to set it to another next");
//                 // }
//                 next_continuation_id = next;
//             }
//             CoroutineState::Active => {
//                 let function_id = coroutine.read().unwrap().entrypoint; // resolve function from index
//                 let function = env
//                     .inner()
//                     .indirect_function_table_lookup(&mut store, function_id.unwrap())
//                     // .map_err(Errno::from)
//                     .expect("Function not found in table");
//                 // Start the coroutine
//                 let continuation = coroutine
//                     .read()
//                     .unwrap()
//                     .resumer
//                     .expect("Next coroutine has no resumer");
//                 let resumer = function.call_resume(&mut store, continuation); // TODO: Handle params

//                 let err = match resumer {
//                     Ok(result) => {
//                         if is_context_main {
//                             // TODO: This is stupid
//                             return Ok(Errno::Success);
//                         }
//                         panic!("Coroutine entrypoint returned normally, which is not supported");
//                     }
//                     Err(err) => err,
//                 };
//                 let Some((continuation_ref, next)) = err.to_continuation_ref() else {
//                     if is_context_main {
//                         // TODO: This is stupid
//                         return Err(err);
//                     }
//                     panic!(
//                         "Coroutine entrypoint did not return a continuation {:?}",
//                         err
//                     );
//                 };

//                 // let Err(err) = resumer else {
//                 //     panic!("Coroutine returned normally, which is not supported");
//                 // };
//                 // let Some((resumable, next)) = err.to_continuation() else {
//                 //     panic!("Coroutine did not return a continuation {:?}", err);
//                 //     // return Err(err.into());
//                 // };
//                 // // It may actually be possible to get a different continuation here, if the coroutine called a wasm function via a syscall
//                 // assert_eq!(continuation, resumable);
//                 coroutine.write().unwrap().resumer = Some(continuation_ref);

//                 // let mut next_coroutine = env.next_coroutine.write().unwrap();
//                 // if next_coroutine.is_some() {
//                 //     unreachable!("Next coroutine is set when we want to set it to another next");
//                 // }
//                 // *next_coroutine = Some(next);
//                 next_continuation_id = next;
//             }
//             CoroutineState::Deleted | CoroutineState::Failed => {
//                 panic!("Switching to deleted or failed coroutine is not supported");
//                 // return Err(WasiError::Exit(Errno::Inval.into()));
//             }
//         }
//     }

//     // wasi_try_mem_ok!(new_coroutine_ptr.write(&memory, new_coroutine_id));
//     unreachable!();

//     Ok(Errno::Success)
}

/// ### `coroutine_new()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn continuation_new<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    new_coroutine_ptr: WasmPtr<u32, M>,
    entrypoint: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();
    let tag = Tag::new(&mut store.as_store_mut(), [Type::I64]);

    let new_coroutine_id = env
        .next_coroutine_id
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // TODO: Support unstarted continuation references
    let new_coroutine = CoroutineStack {
        entrypoint: Some(entrypoint),
        state: CoroutineState::Created,
        resumer: None,
        parent: None,
    };

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let mut coroutines = env.coroutines.write().unwrap();
    coroutines.insert(new_coroutine_id, Arc::new(RwLock::new(new_coroutine)));
    new_coroutine_ptr
        .write(&memory, new_coroutine_id as u32)
        .unwrap();

    Ok(Errno::Success)
}

/// ### `coroutine_switch()`
#[instrument(level = "trace", skip(ctx), ret)]
pub fn continuation_switch(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    next_coroutine: u32,
) -> Result<Errno, RuntimeError> {
    match WasiEnv::do_pending_operations(&mut ctx) {
        Ok(()) => {}
        Err(e) => return Err(RuntimeError::user(Box::new(e))),
    }

    let (env, mut store) = ctx.data_and_store_mut();
    // let mut coroutines = env.coroutines.write().unwrap();
    // let continuation = coroutines
    //     .get_mut(&(next_coroutine as u64))
    //     .unwrap();
    // let continuation: std::sync::RwLockReadGuard<'_, CoroutineStack> = continuation.read().unwrap();
    // let Some(continuation) = &continuation.resumer else {
    //     panic!("Switching to coroutine that has no resumer");
    // };

    // TODO: Creating new tag every time is wrong, but we currently dont check tags
    let tag = Tag::new(&mut store.as_store_mut(), [Type::I64]);
    let continuation = Continuation::new(&mut store.as_store_mut(), &tag, &[Value::I64(next_coroutine as i64)]);
    let (env, store) = ctx.data_and_store_mut();
    Err(RuntimeError::continuation(
        &store.as_store_ref(),
        continuation,
    ))
}
