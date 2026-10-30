//! Threaded C1 regressions. This module is linked only by the source-built test artifact.
use crate::memory::{ForeignMemory, ReadOnly};
use std::{
    future::Future,
    pin::pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    task::{Context, Poll, Waker},
};
use wasixv2_host::{
    Executor,
    browser::{BrowserExecutor, ObjectHooks, block_on, set_object_hooks},
};
use wasixv2_syscalls::{Fault, PageAccess, Restricted, UserMemory};
use wasm_bindgen::prelude::*;
use wasmer::{Memory, MemoryType, Store};

#[wasm_bindgen]
pub fn configure() {
    set_object_hooks(ObjectHooks {
        prepare: || {
            Ok(wasmer::js::prepare_shared_object_message(
                JsValue::UNDEFINED,
            ))
        },
        receive: |envelope| {
            // SAFETY: B2 delivers envelopes only from this trusted runtime.
            unsafe {
                wasmer::js::receive_shared_object_message(envelope)?;
            }
            Ok(())
        },
        collect: wasmer::js::collect_shared_objects,
        teardown: wasmer::js::collect_shared_objects,
    });
}

fn immediate<T>(future: impl Future<Output = T>) -> T {
    // Same completion requirement as kernel/src/mm/retained.rs:sync_copy.
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("retained foreign copies must complete immediately"),
    }
}

fn parity(memory: &ForeignMemory, access: &PageAccess) {
    assert_eq!(immediate(memory.size()), 65536);
    assert_eq!(immediate(memory.write(64, b"guest")), Ok(()));
    let mut buf = [0; 5];
    assert_eq!(immediate(memory.read(64, &mut buf)), Ok(()));
    assert_eq!(&buf, b"guest");
    for (at, len) in [(65535, 2), (u32::MAX, 2), (65537, 0)] {
        assert_eq!(immediate(memory.read(at, &mut buf[..len])), Err(Fault));
        assert_eq!(immediate(memory.write(at, &buf[..len])), Err(Fault));
    }
    assert_eq!(immediate(memory.read(65536, &mut [])), Ok(()));
    assert_eq!(immediate(memory.write(65536, &[])), Ok(()));
    assert_eq!(immediate(memory.read(0, &mut buf)), Err(Fault));
    assert_eq!(immediate(memory.force_read(0, &mut buf)), Ok(()));
    assert_eq!(immediate(memory.force_write(0, &buf)), Err(Fault));
    access.set(vec![Restricted {
        range: 64..128,
        readable: false,
    }]);
    assert_eq!(immediate(memory.read(64, &mut buf)), Err(Fault));
    assert_eq!(immediate(memory.write(64, &buf)), Err(Fault));
    assert_eq!(immediate(memory.force_read(64, &mut buf)), Ok(()));
    assert_eq!(immediate(memory.force_write(64, &buf)), Ok(()));
    access.set(vec![Restricted {
        range: 64..128,
        readable: true,
    }]);
    assert_eq!(immediate(memory.read(64, &mut buf)), Ok(()));
    assert_eq!(immediate(memory.write(64, &buf)), Err(Fault));
    assert_eq!(immediate(memory.compare_exchange_u32(64, 0, 1)), Err(Fault));
    access.set(vec![]);
    memory.read_only.0.write().unwrap().push(64..128);
    assert_eq!(immediate(memory.read(64, &mut buf)), Ok(()));
    assert_eq!(immediate(memory.force_write(64, &buf)), Err(Fault));
    assert_eq!(immediate(memory.zero_pages(64, 4)), Err(Fault));
    assert_eq!(immediate(memory.compare_exchange_u32(64, 0, 1)), Err(Fault));
    memory.read_only.0.write().unwrap().clear();
    immediate(memory.write(64, &u32::MAX.to_le_bytes())).unwrap();
    assert_eq!(
        immediate(memory.compare_exchange_u32(64, u32::MAX, 7)),
        Ok(u32::MAX)
    );
    assert_eq!(
        immediate(memory.compare_exchange_u32(64, u32::MAX, 8)),
        Ok(7)
    );
    for at in [0, 65, 65536, 0xfffffffc] {
        assert_eq!(immediate(memory.compare_exchange_u32(at, 0, 1)), Err(Fault));
    }
    assert_eq!(immediate(memory.zero_pages(0, 4)), Err(Fault));
    assert_eq!(immediate(memory.zero_pages(65535, 4)), Err(Fault));
    assert_eq!(immediate(memory.zero_pages(64, 8)), Ok(()));
    assert_eq!(immediate(memory.read(64, &mut buf)), Ok(()));
    assert_eq!(buf, [0; 5]);
}

#[wasm_bindgen]
pub fn local_copies() -> u32 {
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(4), true)).unwrap();
    let shared = memory.as_shared(&store).unwrap();
    assert!(shared.local_js_memory().is_some());
    let access = Arc::new(PageAccess::default());
    let foreign = ForeignMemory::new(shared, 16, Arc::default(), Some(access.clone()));
    parity(&foreign, &access);
    memory.grow(&mut store, 1).unwrap();
    assert_eq!(immediate(foreign.size()), 131072);
    assert_eq!(immediate(foreign.write(65536, b"grown")), Ok(()));
    let mut buf = [0; 5];
    memory.view(&store).read(65536, &mut buf).unwrap();
    assert_eq!(&buf, b"grown");
    1
}

#[derive(Default)]
struct Remote {
    memory: Mutex<Option<Arc<ForeignMemory>>>,
    command: AtomicU32,
    stage: AtomicU32,
    root: Mutex<Option<Waker>>,
    target: Mutex<Option<Waker>>,
}
impl Remote {
    fn signal(&self, stage: u32) {
        self.stage.store(stage, Ordering::Release);
        if let Some(waker) = self.root.lock().unwrap().take() {
            waker.wake();
        }
    }
    fn stage(&self, stage: u32) {
        block_on(std::future::poll_fn(|cx| {
            *self.root.lock().unwrap() = Some(cx.waker().clone());
            if self.stage.load(Ordering::Acquire) >= stage {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }));
    }
    fn command(&self, command: u32) {
        self.command.store(command, Ordering::Release);
        if let Some(waker) = self.target.lock().unwrap().take() {
            waker.wake();
        }
    }
}

#[wasm_bindgen]
pub fn remote_copies() -> u32 {
    let executor = BrowserExecutor::new(1, 0).unwrap();
    let state = Arc::new(Remote::default());
    let target = state.clone();
    let access = Arc::new(PageAccess::default());
    let target_access = access.clone();
    executor
        .spawn_thread(Box::new(move || {
            Box::pin(async move {
                let mut store = Store::default();
                let memory = Memory::new(&mut store, MemoryType::new(1, Some(4), true)).unwrap();
                let shared = memory.as_shared(&store).unwrap();
                *target.memory.lock().unwrap() = Some(Arc::new(ForeignMemory::new(
                    shared,
                    16,
                    Arc::<ReadOnly>::default(),
                    Some(target_access),
                )));
                let module = wasmer::Module::new(
                    &store,
                    wat::parse_str(include_str!("target.wat")).unwrap(),
                )
                .unwrap();
                let instance = wasmer::Instance::new(
                    &mut store,
                    &module,
                    &wasmer::imports! { "env" => { "memory" => memory.clone() } },
                )
                .unwrap();
                let tick = instance.exports.get_function("tick").unwrap();
                target.signal(1);
                // Actively execute on this worker: no event-loop handling of copy requests.
                while target.command.load(Ordering::Acquire) == 0 {
                    tick.call(&mut store, &[]).unwrap();
                }
                memory.grow(&mut store, 1).unwrap();
                target.signal(2);
                std::future::poll_fn(|cx| {
                    *target.target.lock().unwrap() = Some(cx.waker().clone());
                    if target.command.load(Ordering::Acquire) >= 2 {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                // Drop owner/store, like old-mm teardown after exec. The retained foreign
                // handle and JS service still own the guest Memory (Linux mm/memory.c:
                // __access_remote_vm; io_uring/rsrc.c:io_buffer_unmap).
                target.memory.lock().unwrap().take();
                drop(instance);
                drop(module);
                drop(memory);
                drop(store);
                target.signal(3);
            })
        }))
        .unwrap();
    state.stage(1);
    let foreign = state.memory.lock().unwrap().clone().unwrap();
    assert!(foreign.memory.local_js_memory().is_none());
    parity(&foreign, &access);
    state.command(1);
    state.stage(2);
    // Target is now asleep in Atomics.wait, yet every copy completes immediately.
    assert_eq!(immediate(foreign.size()), 131072);
    immediate(foreign.write(65536, b"sleep")).unwrap();
    let mut buf = [0; 5];
    immediate(foreign.read(65536, &mut buf)).unwrap();
    assert_eq!(&buf, b"sleep");
    state.command(2);
    state.stage(3);
    immediate(foreign.read(65536, &mut buf)).unwrap();
    assert_eq!(&buf, b"sleep");
    drop(foreign); // final registration release may run on a different worker.
    1
}
