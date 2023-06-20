use std::{
    sync::Arc,
    task::{RawWaker, RawWakerVTable, Waker},
};
use tokio::sync::mpsc;

use wasmer_wasix_types::wasi::WakerId;

use super::WasiState;

pub(crate) struct WasiWaker {
    id: WakerId,
    tx: mpsc::UnboundedSender<(WakerId, bool)>,
}
impl WasiWaker {
    pub(crate) fn new(id: WakerId, tx: &mpsc::UnboundedSender<(WakerId, bool)>) -> Self {
        Self { id, tx: tx.clone() }
    }

    fn wake_now(&self) {
        self.tx.send((self.id, true)).ok();
    }
}
impl Drop for WasiWaker {
    fn drop(&mut self) {
        self.tx.send((self.id, false)).ok();
    }
}

fn wasi_waker_wake(s: &WasiWaker) {
    let waker_arc = unsafe { Arc::from_raw(s) };
    waker_arc.wake_now();
}

fn wasi_waker_clone(s: &WasiWaker) -> RawWaker {
    let arc = unsafe { Arc::from_raw(s) };
    std::mem::forget(arc.clone());
    RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE)
}

const VTABLE: RawWakerVTable = unsafe {
    RawWakerVTable::new(
        |s| wasi_waker_clone(&*(s as *const WasiWaker)), // clone
        |s| wasi_waker_wake(&*(s as *const WasiWaker)),  // wake
        |s| (*(s as *const WasiWaker)).wake_now(),       // wake by ref (don't decrease refcount)
        |s| drop(Arc::from_raw(s as *const WasiWaker)),  // decrease refcount
    )
};

fn wasi_waker_into_waker(s: *const WasiWaker) -> Waker {
    let raw_waker = RawWaker::new(s as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw_waker) }
}

pub(crate) fn conv_waker_id(state: &WasiState, id: WakerId) -> Waker {
    tracing::trace!("waker registered {id}");
    let waker = Arc::new(state.wakers.create_waker(id));
    wasi_waker_into_waker(Arc::into_raw(waker))
}
