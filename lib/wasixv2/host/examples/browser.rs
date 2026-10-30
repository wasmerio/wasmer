//! Threaded browser executor regression harness, built from source by tests/browser/build.sh.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::time::Duration;
    use std::{
        cell::{Cell, RefCell},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU32, Ordering},
        },
        task::{Poll, Waker},
    };
    use wasixv2_host::{
        BrowserHost, ByteSource, Executor, Host, WebHost,
        browser::{BrowserExecutor, ObjectHooks, block_on, set_object_hooks},
    };
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(
        inline_js = "export function guestAdd() { Atomics.add(new Int32Array(globalThis.guest.buffer), 0, 1); } export function guestGet() { return Atomics.load(new Int32Array(globalThis.guest.buffer), 0); }"
    )]
    extern "C" {
        fn guestAdd();
        fn guestGet() -> u32;
    }

    #[wasm_bindgen(
        inline_js = "export function noJSPI() { return typeof WebAssembly.Suspending === 'undefined' && typeof WebAssembly.promising === 'undefined'; }"
    )]
    extern "C" {
        fn noJSPI() -> bool;
    }

    static TLS_DROPS: AtomicU32 = AtomicU32::new(0);
    struct TrackedTls(Vec<u8>);
    impl Drop for TrackedTls {
        fn drop(&mut self) {
            assert_eq!(self.0.len(), 128);
            TLS_DROPS.fetch_add(1, Ordering::Release);
        }
    }
    thread_local! { static TRACKED: RefCell<Option<TrackedTls>> = const { RefCell::new(None) }; }

    #[wasm_bindgen]
    pub fn tls_counter_address() -> u32 {
        &TLS_DROPS as *const _ as u32
    }

    thread_local! { static TLS: Cell<u32> = const { Cell::new(0) }; }

    #[wasm_bindgen]
    pub fn configure() {
        TRACKED.with_borrow_mut(|tracked| *tracked = Some(TrackedTls(vec![42; 128])));
        set_object_hooks(ObjectHooks {
            prepare: || {
                if js_sys::Reflect::get(&js_sys::global(), &"rejectEnvelope".into())?
                    == JsValue::TRUE
                {
                    Ok("reject".into())
                } else {
                    js_sys::Reflect::get(&js_sys::global(), &"guest".into())
                }
            },
            receive: |envelope| {
                if envelope.as_string().as_deref() == Some("reject") {
                    return Err("test envelope rejection".into());
                }
                js_sys::Reflect::set(&js_sys::global(), &"guest".into(), &envelope)?;
                Ok(())
            },
            collect: || {
                js_sys::Reflect::delete_property(&js_sys::global(), &"guest".into()).unwrap();
            },
            teardown: || {
                js_sys::Reflect::delete_property(&js_sys::global(), &"guest".into()).unwrap();
            },
        });
    }

    #[derive(Default)]
    struct Shared {
        count: AtomicU32,
        waker: Mutex<Option<Waker>>,
        released: AtomicBool,
        waiters: Mutex<Vec<Waker>>,
    }

    /// This runs on a synchronously parked root and B2 service/task workers, with
    /// no JSPI. Timers must continue independently of all of those event loops.
    #[wasm_bindgen]
    pub fn timer_smoke() -> u32 {
        let executor = BrowserExecutor::new(2, 2).unwrap();
        let host = BrowserHost::new(executor.clone()).unwrap();
        let service = host.services();
        let before = host.monotonic();
        let shared = Arc::new(Shared::default());
        let busy = shared.clone();
        let busy_host = host.clone();
        executor
            .spawn_thread(Box::new(move || {
                Box::pin(async move {
                    // The worker's time origin is later than root's, but its clock cannot
                    // go backwards. Busy Rust wasm occupies its event loop like a guest.
                    assert!(busy_host.monotonic() >= before);
                    while !busy.released.load(Ordering::Acquire) {
                        std::hint::spin_loop();
                    }
                    busy.count.fetch_add(1, Ordering::Release);
                    let waker = busy.waker.lock().unwrap().take();
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                })
            }))
            .unwrap();
        #[derive(Default)]
        struct Input {
            ready: AtomicBool,
            waker: Mutex<Option<Waker>>,
        }
        impl ByteSource for Input {
            fn poll_read(
                &self,
                cx: &mut std::task::Context<'_>,
                buf: &mut [u8],
            ) -> Poll<std::io::Result<usize>> {
                if buf.is_empty() {
                    return Poll::Ready(Ok(0));
                }
                *self.waker.lock().unwrap() = Some(cx.waker().clone());
                if self.ready.swap(false, Ordering::AcqRel) {
                    buf[0] = 3; // raw Ctrl-C; line discipline/signals belong to the kernel.
                    Poll::Ready(Ok(1))
                } else {
                    Poll::Pending
                }
            }
        }
        let input = Arc::new(Input::default());
        let read_input = input.clone();
        let done = shared.clone();
        executor
            .services()
            .spawn_thread(Box::new(move || {
                Box::pin(async move {
                    let mut byte = [0];
                    assert_eq!(
                        std::future::poll_fn(|cx| read_input.poll_read(cx, &mut byte))
                            .await
                            .unwrap(),
                        1
                    );
                    assert_eq!(byte, [3]);
                    done.released.store(true, Ordering::Release);
                })
            }))
            .unwrap();
        executor
            .services()
            .spawn_thread(Box::new(move || {
                Box::pin(async move {
                    let start = service.monotonic();
                    service
                        .sleep_until_range(
                            start + Duration::from_millis(30),
                            Duration::from_millis(10),
                        )
                        .await;
                    assert!(service.monotonic() >= start + Duration::from_millis(30));
                    input.ready.store(true, Ordering::Release);
                    let waker = input.waker.lock().unwrap().take();
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                })
            }))
            .unwrap();
        block_on(std::future::poll_fn(|cx| {
            *shared.waker.lock().unwrap() = Some(cx.waker().clone());
            if shared.count.load(Ordering::Acquire) == 1 {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }));
        block_on(async {
            let deadline = host.monotonic() + Duration::from_millis(10);
            host.sleep_until(deadline).await;
            assert!(host.monotonic() >= deadline);
            // The first poll registers; dropping synchronously removes the far alarm.
            let mut far = host.sleep(Duration::from_secs(3_000_000));
            assert!(
                far.as_mut()
                    .poll(&mut std::task::Context::from_waker(Waker::noop()))
                    .is_pending()
            );
            drop(far);
            // Sending cancellation and a new earlier sleep cannot revive the old entry.
            host.sleep(Duration::from_millis(1)).await;
            host.sleep_until(Duration::ZERO).await;
        });
        let mut bytes = vec![0; 65536 * 2 + 17];
        host.random(&mut bytes);
        assert!(bytes.iter().any(|byte| *byte != 0));
        assert!(bytes[65536 * 2..].iter().any(|byte| *byte != 0));
        assert!(host.thread_cpu_clock().is_none());
        assert!(host.thread_cpu_time().is_none());
        // Exercise simple WebHost cancellation on this worker without awaiting its JS loop.
        let roots = wasm_bindgen::externref_heap_live_count();
        let mut local = WebHost.sleep(Duration::from_secs(3600));
        assert!(
            local
                .as_mut()
                .poll(&mut std::task::Context::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(local);
        assert_eq!(wasm_bindgen::externref_heap_live_count(), roots);
        1
    }

    #[wasm_bindgen]
    pub fn sctp_smoke() -> u32 {
        use wasixv2_abi::{Errno, MsgFlags, SctpOption, SocketLevel};
        use wasixv2_vnet::{Domain, Net, NetConfig, Network, SendMsg, SocketKind};
        let net = Net::new(NetConfig::default(), None);
        let listener = net
            .socket(Domain::Inet, SocketKind::SeqPacket, 132, 1)
            .unwrap();
        let client = net
            .socket(Domain::Inet, SocketKind::SeqPacket, 132, 1)
            .unwrap();
        let address = "127.0.0.1:5000".parse().unwrap();
        listener.bind(address, true).unwrap();
        listener.listen(4).unwrap();
        client
            .set_option(
                SocketLevel::SOL_SCTP.0,
                SctpOption::SCTP_AUTOCLOSE.0,
                &1u32.to_le_bytes(),
            )
            .unwrap();
        assert_eq!(client.connect(address), Err(Errno::EINPROGRESS));
        let before = client.sctp().unwrap().autoclose_expire().unwrap();
        assert!(before <= Duration::from_secs(1) && !before.is_zero());
        assert_eq!(
            client.send(SendMsg {
                data: b"sctp",
                to: Some(address),
                flags: MsgFlags(0),
                control: &[],
                net_raw: false,
                now: Duration::ZERO,
            }),
            Ok(4)
        );
        let mut buffer = [0; 4];
        assert_eq!(listener.recv(&mut buffer, MsgFlags(0)).unwrap().len, 4);
        assert_eq!(&buffer, b"sctp");
        let host = BrowserHost::new(BrowserExecutor::new(1, 1).unwrap()).unwrap();
        block_on(host.sleep(Duration::from_millis(1050)));
        assert_eq!(client.sctp().unwrap().autoclose_expire(), None);
        1
    }

    #[wasm_bindgen]
    pub fn admission_error() -> u32 {
        struct DropCount(Arc<AtomicU32>);
        impl Drop for DropCount {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Release);
            }
        }
        let drops = Arc::new(AtomicU32::new(0));
        let count = DropCount(drops.clone());
        let executor = BrowserExecutor::new(1, 1).unwrap();
        assert!(
            executor
                .spawn_thread(Box::new(move || {
                    drop(count);
                    Box::pin(async { panic!("rejected closure must never run") })
                }))
                .is_err()
        );
        assert_eq!(drops.load(Ordering::Acquire), 1);
        1
    }

    #[wasm_bindgen]
    pub fn nested_spawn() -> u32 {
        #[derive(Default)]
        struct Chain {
            count: AtomicU32,
            root: Mutex<Option<Waker>>,
            parent: Mutex<Option<Waker>>,
        }
        let executor = BrowserExecutor::new(2, 1).unwrap();
        let parent_executor = executor.clone();
        let chain = Arc::new(Chain::default());
        let parent = chain.clone();
        executor
            .spawn_thread(Box::new(move || {
                Box::pin(async move {
                    let child = parent.clone();
                    parent_executor
                        .spawn_thread(Box::new(move || {
                            Box::pin(async move {
                                child.count.store(1, Ordering::Release);
                                let waker = child.parent.lock().unwrap().take();
                                if let Some(waker) = waker {
                                    waker.wake();
                                }
                            })
                        }))
                        .unwrap();
                    std::future::poll_fn(|cx| {
                        *parent.parent.lock().unwrap() = Some(cx.waker().clone());
                        if parent.count.load(Ordering::Acquire) == 1 {
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    })
                    .await;
                    parent.count.store(2, Ordering::Release);
                    let waker = parent.root.lock().unwrap().take();
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                })
            }))
            .unwrap();
        block_on(std::future::poll_fn(|cx| {
            *chain.root.lock().unwrap() = Some(cx.waker().clone());
            if chain.count.load(Ordering::Acquire) == 2 {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }));
        2
    }

    #[wasm_bindgen]
    pub fn worker_trap() {
        let executor = BrowserExecutor::new(1, 1).unwrap();
        executor
            .spawn_thread(Box::new(|| {
                Box::pin(async { panic!("accepted worker failure") })
            }))
            .unwrap();
        block_on(std::future::pending::<()>());
    }

    #[wasm_bindgen]
    pub fn smoke() -> u32 {
        assert!(noJSPI());
        let executor = BrowserExecutor::new(2, 1).unwrap();
        TLS.with(|tls| tls.set(99));
        let shared = Arc::new(Shared::default());
        block_on(async {
            // Start two workers while the root parks. Both use one Rust Arc and
            // a distinct shared guest WebAssembly.Memory received in the envelope.
            for _ in 0..2 {
                let shared = shared.clone();
                executor
                    .spawn_thread(Box::new(move || {
                        Box::pin(async move {
                            assert!(noJSPI());
                            assert_eq!(TLS.with(Cell::get), 0);
                            TLS.with(|tls| tls.set(7));
                            std::future::poll_fn(|cx| {
                                let mut waiters = shared.waiters.lock().unwrap();
                                if shared.released.load(Ordering::Acquire) {
                                    Poll::Ready(())
                                } else {
                                    waiters.push(cx.waker().clone());
                                    Poll::Pending
                                }
                            })
                            .await;
                            for _ in 0..1000 {
                                guestAdd();
                            }
                            shared.count.fetch_add(1, Ordering::Release);
                            let waker = shared.waker.lock().unwrap().take();
                            if let Some(waker) = waker {
                                waker.wake();
                            }
                            TLS.with(|tls| tls.set(0));
                        })
                    }))
                    .unwrap();
            }
            // Service work has independent reserved capacity even with every task
            // worker occupied and waiting. This is also the timer/TTY ThreadStart shape.
            let service_shared = shared.clone();
            executor
                .services()
                .spawn_thread(Box::new(move || {
                    Box::pin(async move {
                        service_shared.released.store(true, Ordering::Release);
                        let waiters = std::mem::take(&mut *service_shared.waiters.lock().unwrap());
                        for waker in waiters {
                            waker.wake();
                        }
                    })
                }))
                .unwrap();
            std::future::poll_fn(|cx| {
                *shared.waker.lock().unwrap() = Some(cx.waker().clone());
                if shared.count.load(Ordering::Acquire) == 2 {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            let flag = std::rc::Rc::new(Cell::new(false));
            let local_flag = flag.clone();
            executor.spawn_local(Box::pin(async move { local_flag.set(true) }));
            block_on(std::future::poll_fn(|_| {
                if flag.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }));
        });
        assert_eq!(TLS.with(Cell::get), 99);
        assert_eq!(shared.count.load(Ordering::Acquire), 2);
        assert_eq!(guestGet(), 2000);
        2000
    }
}
