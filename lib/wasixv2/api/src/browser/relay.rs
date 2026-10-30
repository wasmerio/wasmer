//! Browser transport glue for B5's classic relay; Linux socket policy stays in vnet.
use crate::{Net, NetConfig};
use futures_util::task::AtomicWaker;
use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU32, Ordering},
    },
    task::{Context, Poll},
};
use wasixv2_abi::Errno;
use wasixv2_host::{Executor, browser::BrowserExecutor};
use wasixv2_vnet_web::{Capabilities, Classic, Config, FrameTransport, WebBackend};
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, Waiter};
use wasm_bindgen::prelude::*;

const QUEUE_BYTES: usize = wasixv2_vnet_web::codec::MAX_FRAME * 2;
const QUEUE_RECORDS: usize = 128;
static NEXT: AtomicU32 = AtomicU32::new(1);
static CHANNELS: LazyLock<Mutex<HashMap<u32, Weak<Shared>>>> = LazyLock::new(Mutex::default);
#[derive(Default)]
struct State {
    opened: bool,
    sending: bool,
    incoming: VecDeque<Vec<u8>>,
    bytes: usize,
    closed: Option<Errno>,
    done: bool,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: AtomicWaker,
    changed: WaitQueue,
}
impl Shared {
    fn fail(&self, error: Errno) {
        {
            let mut state = self.state.lock().unwrap();
            state.closed.get_or_insert(error);
        }
        self.wake.wake();
    }
}
fn channel(id: u32) -> Option<Arc<Shared>> {
    CHANNELS.lock().unwrap().get(&id).and_then(Weak::upgrade)
}

#[wasm_bindgen(module = "/src/browser/relay-bridge.js")]
extern "C" {
    #[wasm_bindgen(catch,js_name=relayOpen)]
    fn open(id: u32, url: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch,js_name=relaySend)]
    fn send(id: u32, bytes: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name=relayClose)]
    fn close(id: u32);
}

/// Completion exports run briefly on the independent I/O worker, never on a guest
/// worker's Promise queue. Wakes carry B2's shared scheduler words only.
#[wasm_bindgen]
pub fn browser_relay_state(id: u32, event: u32) {
    let Some(shared) = channel(id) else {
        return;
    };
    {
        let mut state = shared.state.lock().unwrap();
        match event {
            1 => state.opened = true,
            2 => state.sending = false,
            _ => {
                state.closed.get_or_insert(Errno::ENETDOWN);
            }
        }
    }
    shared.wake.wake();
}

#[wasm_bindgen]
pub fn browser_relay_frame(id: u32, bytes: &[u8]) -> bool {
    let Some(shared) = channel(id) else {
        return false;
    };
    let accepted = {
        let mut state = shared.state.lock().unwrap();
        if state.closed.is_some() {
            return false;
        }
        if bytes.len() > wasixv2_vnet_web::codec::MAX_FRAME
            || state.bytes + bytes.len() > QUEUE_BYTES
            || state.incoming.len() >= QUEUE_RECORDS
        {
            state.closed = Some(Errno::ENOBUFS);
            false
        } else {
            state.bytes += bytes.len();
            state.incoming.push_back(bytes.to_vec());
            true
        }
    };
    shared.wake.wake();
    accepted
}

struct Frames {
    id: u32,
    shared: Arc<Shared>,
}
impl FrameTransport for Frames {
    fn poll_send(&mut self, cx: &mut Context<'_>, frame: &[u8]) -> Poll<Result<(), Errno>> {
        self.shared.wake.register(cx.waker());
        {
            let mut state = self.shared.state.lock().unwrap();
            if let Some(error) = state.closed {
                return Poll::Ready(Err(error));
            }
            if !state.opened || state.sending {
                return Poll::Pending;
            }
            if frame.len() > wasixv2_vnet_web::codec::MAX_FRAME {
                return Poll::Ready(Err(Errno::EMSGSIZE));
            }
            state.sending = true;
        }
        if send(self.id, frame).is_err() {
            self.shared.fail(Errno::ENETDOWN);
            return Poll::Ready(Err(Errno::ENETDOWN));
        }
        Poll::Ready(Ok(()))
    }
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<Vec<u8>, Errno>> {
        self.shared.wake.register(cx.waker());
        let mut state = self.shared.state.lock().unwrap();
        if let Some(frame) = state.incoming.pop_front() {
            state.bytes -= frame.len();
            return Poll::Ready(Ok(frame));
        }
        match state.closed {
            Some(error) => Poll::Ready(Err(error)),
            None => Poll::Pending,
        }
    }
    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<Errno> {
        self.shared.wake.register(cx.waker());
        self.shared
            .state
            .lock()
            .unwrap()
            .closed
            .map_or(Poll::Pending, Poll::Ready)
    }
    fn close(&mut self) {
        close(self.id);
        CHANNELS.lock().unwrap().remove(&self.id);
    }
}

/// Owns the browser relay session. Stop the Machine first, then await shutdown;
/// the driver fails pending sockets/lookups and releases the WebSocket callbacks.
pub struct Relay {
    id: u32,
    shared: Arc<Shared>,
    backend: Arc<WebBackend>,
}
impl Relay {
    /// Provider operations whose bind-address completion is asynchronous. Drive them on a
    /// task worker; the session's independent I/O worker continues the relay protocol.
    pub fn backend(&self) -> &WebBackend {
        &self.backend
    }

    pub async fn shutdown(&self) {
        self.shared.fail(Errno::ENETDOWN);
        let waiter = Waiter::new();
        let _registration = waiter.register(&self.shared.changed, PollMask::empty());
        loop {
            if self.shared.state.lock().unwrap().done {
                return;
            }
            waiter.sleep(&NoInterrupt, None).await;
        }
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.shared.fail(Errno::ENETDOWN);
        close(self.id);
    }
}

/// Connect the classic binary WebSocket relay on the bootstrap's independent I/O
/// worker. No replay/reconnect is attempted; a new session creates a new Net.
pub fn connect(
    url: &str,
    config: NetConfig,
    limits: Config,
    capabilities: Capabilities,
    executor: &BrowserExecutor,
) -> io::Result<(Arc<Net>, Relay)> {
    let (backend, driver) = WebBackend::new(limits, capabilities)
        .map_err(|errno| io::Error::from_raw_os_error(errno.0))?;
    let network = Net::new(config, Some(backend.clone()));
    let id = NEXT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| io::Error::other("relay IDs exhausted"))?;
    let shared = Arc::new(Shared::default());
    CHANNELS.lock().unwrap().insert(id, Arc::downgrade(&shared));
    if let Err(error) = open(id, url) {
        CHANNELS.lock().unwrap().remove(&id);
        return Err(io::Error::other(format!("relay bootstrap: {error:?}")));
    }
    let frames = Frames {
        id,
        shared: shared.clone(),
    };
    let done = shared.clone();
    if let Err(error) = executor.services().spawn_thread(Box::new(move || {
        Box::pin(async move {
            let _ = driver.run(Classic::new(frames, capabilities)).await;
            done.state.lock().unwrap().done = true;
            done.changed.wake(PollMask::empty());
        })
    })) {
        close(id);
        CHANNELS.lock().unwrap().remove(&id);
        return Err(error);
    }
    Ok((
        network,
        Relay {
            id,
            shared,
            backend,
        },
    ))
}
