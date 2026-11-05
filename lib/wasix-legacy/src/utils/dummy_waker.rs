/// A "mock" no-op [`std::task::Waker`] implementation.
///
/// Needed for polling futures outside of an async runtime, since the `poll()`
/// functions requires a `[std::task::Context]` with a supplied waker.
#[derive(Debug, Clone)]
pub struct WasiDummyWaker;

impl cooked_waker::Wake for WasiDummyWaker {
    fn wake(self) {}
}

impl cooked_waker::WakeRef for WasiDummyWaker {
    fn wake_by_ref(&self) {}
}

unsafe impl cooked_waker::ViaRawPointer for WasiDummyWaker {
    type Target = ();
    fn into_raw(self) -> *mut () {
        std::ptr::null_mut()
    }
    unsafe fn from_raw(_ptr: *mut ()) -> Self {
        WasiDummyWaker
    }
}
