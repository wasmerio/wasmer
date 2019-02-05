use crate::sys;
use parking_lot::Mutex;
use std::sync::atomic::AtomicUsize;

// Remove this attribute once this is used.
#[allow(dead_code)]
pub struct SharedStaticMemory {
    memory: sys::Memory,
    current: AtomicUsize,
    lock: Mutex<()>,
}
