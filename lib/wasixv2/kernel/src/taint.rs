//! The kernel's taint flags (`kernel/panic.c`), as `/proc/sys/kernel/tainted` shows them.
//!
//! Nothing in this kernel taints it: no module loads, no oops or warning happens. The flags
//! start clear and change only when a process with `CAP_SYS_ADMIN` sets some through the file,
//! as `proc_taint` lets it, so test suites that read them before and after a test (LTP's
//! `tst_taint`) see what they expect.

use std::sync::atomic::{AtomicU32, Ordering};

/// `TAINT_FLAGS_COUNT`: the flags Linux knows, bits 0 to 20. A write sets only these.
const FLAGS_COUNT: u32 = 21;

/// `tainted_mask`, one for the machine.
#[derive(Default)]
pub(crate) struct Taint(AtomicU32);

impl Taint {
    /// `get_taint`.
    pub(crate) fn get(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }

    /// `proc_taint`'s write: each known flag set in `flags` is added (`add_taint`), the others
    /// are ignored, and none is ever cleared.
    pub(crate) fn add(&self, flags: u32) {
        self.0
            .fetch_or(flags & ((1 << FLAGS_COUNT) - 1), Ordering::Relaxed);
    }
}
