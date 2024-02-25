use crate::os::task::process::LockableWasiProcessInner;

use super::WasiProcessCheckpoint;

pub(crate) fn do_checkpoint_from_outside(
    process: &LockableWasiProcessInner,
    checkpoint: WasiProcessCheckpoint,
) {
    let mut guard = process.0.lock().unwrap();

    // Initiate the checksum (if one already exists we must wait for it to end
    // before we start the next checksum)

    // TODO: Disabled as this blocks the async runtime
    //while !matches!(guard.checkpoint, WasiProcessCheckpoint::Execute) {
    //    guard = process.1.wait(guard).unwrap();
    //}

    guard.checkpoint = checkpoint;
    for waker in guard.wakers.drain(..) {
        waker.wake();
    }
    process.1.notify_all();
}
