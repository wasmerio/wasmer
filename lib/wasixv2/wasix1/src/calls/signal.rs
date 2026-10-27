//! Signals.
//!
//! wasix-libc keeps its own disposition table and runs every handler, and its default actions,
//! from one export, `__wasm_signal(sig)`. At startup it asks for the dispositions the process
//! inherited (`proc_signals_*`, failing hard if that fails) and names that export
//! (`callback_signal`); from then on it toggles delivery by naming either the export (deliver)
//! or a name it does not export, `__wasm_signal_blocked` (hold signals back).
//!
//! On the kernel side, the first `callback_signal` of a process installs a placeholder handler
//! for every signal libc may handle, so that each one reaches the guest as a handler run, which
//! the instance turns into a call of the named export. Holding back is the calling thread's
//! signal mask: v1 libc never sets one itself (its `sigprocmask` does nothing), so the mask is
//! all-blocked or empty. Old libcs leak one hold in `pthread_create`, which the spawn ends
//! ([`end_spawn_hold`]). Signals ignored at startup stay ignored in the kernel, so that `exec`
//! keeps them ignored as on Linux. Signals whose default action is to ignore them (`SIGCHLD`,
//! `SIGCONT`, `SIGURG`, `SIGWINCH`) keep their default: v1 never delivered them, and a handler
//! would turn every child's exit into `EINTR` in its parent's blocking calls.
//!
//! As in v1, the placeholder has neither `SA_RESTART` (a blocking call a signal interrupts
//! returns `INTR` once `__wasm_signal` ran) nor a mask of its own: it is `SA_NODEFER`, since
//! libc applies `sa_mask` and `SA_NODEFER` itself and must see a signal raised from a handler.

use std::sync::atomic::Ordering;

use wasixv2_abi::{
    ConstSigactionPtr, ConstSigsetPtr, GuestLayout, ItimerWhich, KernelOldItimerval,
    KernelOldItimervalPtr, KernelOldTimeval, SaFlags, Sigaction, SigactionPtr, SignalHandler,
    SigprocmaskHow, Sigset, SigsetPtr,
};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Bool, Errno,
        proc::{Disposition, Signal, SignalDisposition},
    },
    calls::proc::read_bytes,
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// The handler the compat layer installs: anything but `SIG_DFL` and `SIG_IGN`. Delivery to a
/// v1 guest calls the instance's callback export, never a table entry.
const PLACEHOLDER_HANDLER: u32 = 2;

/// The highest signal number.
const NSIG: i32 = 64;

/// Signals whose default action is to ignore them (Linux's `SIG_KERNEL_IGNORE_MASK`).
const DEFAULT_IGNORED: [i32; 4] = [17, 18, 23, 28];

const SIGKILL: i32 = 9;
const SIGSTOP: i32 = 19;
const SIGALRM: u8 = 14;

/// Whether the compat layer installs its placeholder handler for `sig`.
fn delivered(sig: i32) -> bool {
    sig != SIGKILL && sig != SIGSTOP && !DEFAULT_IGNORED.contains(&sig)
}

/// The mask that holds back every signal that can be held back.
fn all_blocked() -> Sigset {
    let bits = !(1u64 << (SIGKILL - 1) | 1u64 << (SIGSTOP - 1));
    Sigset {
        sig: [bits as u32, (bits >> 32) as u32],
    }
}

/// The Linux signal for v1's `sig`. v1 passes Linux's numbers.
fn linux(sig: i32) -> wasixv2_abi::Signal {
    wasixv2_abi::Signal(sig & 0xff)
}

/// The kernel's disposition of `sig` for the calling task's process.
async fn action<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    sig: i32,
) -> V1Result<Sigaction> {
    let old = frame.mem.alloc_mut::<Sigaction>()?;
    frame
        .syscalls
        .rt_sigaction(
            &frame.caller(),
            wasixv2_abi::Signal(sig),
            ConstSigactionPtr::NULL,
            old,
            Sigset::SIZE as u32,
        )
        .await?;
    Ok(frame.mem.take(old))
}

/// Sets `sig`'s handler, with an empty mask and `flags`.
pub(crate) async fn set_handler<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    sig: i32,
    handler: u32,
    flags: SaFlags,
) -> V1Result<()> {
    let act = frame.mem.alloc(&Sigaction {
        sa_handler: handler,
        sa_flags: flags.0,
        sa_mask: Sigset::ZERO,
    })?;
    frame
        .syscalls
        .rt_sigaction(
            &frame.caller(),
            wasixv2_abi::Signal(sig),
            act,
            SigactionPtr::NULL,
            Sigset::SIZE as u32,
        )
        .await?;
    Ok(())
}

/// The disposition the process inherited for every signal v1 can name and a process may
/// change: ignored or default.
///
/// Reporting the defaults too is not only complete: wasix-libc sets up the default action it
/// runs from `__wasm_signal` in its first `sigaction`, and its startup calls `sigaction` once
/// per reported disposition. With nothing reported, a signal that reaches the default action
/// before the program's own first `sigaction` (`abort` without a handler) calls a NULL handler.
async fn inherited<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
) -> V1Result<Vec<SignalDisposition>> {
    let mut dispositions = Vec::new();
    for sig in 1..=31 {
        let Some(signal) = Signal::from_linux(sig) else {
            continue;
        };
        if signal == Signal::SIGKILL || signal == Signal::SIGSTOP {
            continue;
        }
        let disp = match action(frame, sig).await?.sa_handler {
            handler if handler == SignalHandler::SIG_IGN.0 as u32 => Disposition::IGNORE,
            _ => Disposition::DEFAULT,
        };
        dispositions.push(SignalDisposition { sig: signal, disp });
    }
    Ok(dispositions)
}

pub(crate) async fn proc_signals_sizes_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    count: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let n = inherited(frame).await?.len() as u32;
        frame.write(count as u32, &n).await
    })
    .await
}

pub(crate) async fn proc_signals_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    buf: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let mut addr = buf as u32;
        for disposition in inherited(frame).await? {
            frame.write(addr, &disposition).await?;
            addr += SignalDisposition::SIZE as u32;
        }
        Ok(())
    })
    .await
}

/// Installs the placeholder handler for every signal libc may handle, once per process.
async fn install<S: Syscalls + Control + 'static>(frame: &Frame<'_, S>) -> V1Result<()> {
    let proc = frame.proc().await?;
    if proc.proc.signals_installed.load(Ordering::Acquire) {
        return Ok(());
    }
    // The kernel wants a registration before it runs handlers; v1 delivery never uses the
    // table index.
    frame
        .syscalls
        .signal_callback_register(&frame.caller(), 0)
        .await?;
    for sig in (1..=NSIG).filter(|&sig| delivered(sig)) {
        if action(frame, sig).await?.sa_handler != SignalHandler::SIG_IGN.0 as u32 {
            set_handler(frame, sig, PLACEHOLDER_HANDLER, SaFlags::SA_NODEFER).await?;
        }
    }
    proc.proc.signals_installed.store(true, Ordering::Release);
    Ok(())
}

/// `callback_signal(name, name_len)`: names the export that runs signal handlers. A name the
/// module does not export holds signals back until a later call names one it does.
pub(crate) async fn callback_signal<S: Syscalls + Control + 'static>(
    env: Env<S>,
    name: i32,
    name_len: i32,
) -> Result<(), RuntimeError> {
    shim::void(&env, "callback_signal", async |frame| {
        let name = read_bytes(frame, name as u32, name_len as u32).await?;
        let deliver = std::str::from_utf8(&name)
            .is_ok_and(|name| frame.instance().select_signal_callback(name));
        // A module with no callback at all cannot run a handler: signals keep their default
        // actions, as before any `callback_signal`.
        if !deliver && !frame.instance().has_signal_callback() {
            return Ok(());
        }
        install(frame).await?;
        set_mask(frame, deliver).await
    })
    .await
}

/// Sets the calling thread's mask: nothing held back when `deliver`, everything otherwise.
async fn set_mask<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    deliver: bool,
) -> V1Result<()> {
    let mask = match deliver {
        true => Sigset::ZERO,
        false => all_blocked(),
    };
    let mask = frame.mem.alloc(&mask)?;
    frame
        .syscalls
        .rt_sigprocmask(
            &frame.caller(),
            SigprocmaskHow::SIG_SETMASK,
            ConstSigsetPtr::new(mask.addr()),
            SigsetPtr::NULL,
            Sigset::SIZE as u32,
        )
        .await?;
    Ok(())
}

/// Ends the hold `pthread_create` leaves behind in wasix-libc before v2026-07-30.1: it holds
/// signals back (`__block_app_sigs`, which names `__wasm_signal_blocked`) before it spawns the
/// thread, and its `__restore_sigs` is compiled out, so the calling thread, and the thread,
/// which inherits the mask, would never take a signal again: no Ctrl-C, `kill` or `alarm`. As
/// musl restores the caller's mask and starts the thread with it, the spawn restores delivery
/// first, which is what that `__restore_sigs` would have done. Later libcs keep the mask
/// themselves and never hold signals back through the host.
pub(crate) async fn end_spawn_hold<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
) -> V1Result<()> {
    let instance = frame.instance();
    if !instance.blocked() || !instance.select_signal_callback(crate::SIGNAL_CALLBACK) {
        return Ok(());
    }
    set_mask(frame, true).await
}

/// `proc_raise(sig)`: the calling thread, as `raise` does.
pub(crate) async fn proc_raise<S: Syscalls + Control + 'static>(
    env: Env<S>,
    sig: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let caller = frame.caller();
        let pid = frame.syscalls.getpid(&caller).await?;
        let tid = frame.syscalls.gettid(&caller).await?;
        frame.syscalls.tgkill(&caller, pid, tid, linux(sig)).await?;
        Ok(())
    })
    .await
}

/// The tid wasix-libc gives its main thread, which never learns its real one (wasi-libc's
/// convention: it is above every host tid and keeps TID-based locks working).
const LIBC_MAIN_THREAD: i32 = 0x3fff_ffff;

/// `thread_signal(tid, sig)`: a thread of the calling process. libc's main-thread tid names
/// the main thread, whose tid is the pid.
pub(crate) async fn thread_signal<S: Syscalls + Control + 'static>(
    env: Env<S>,
    tid: i32,
    sig: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let caller = frame.caller();
        let pid = frame.syscalls.getpid(&caller).await?;
        let tid = if tid == LIBC_MAIN_THREAD { pid } else { tid };
        frame.syscalls.tgkill(&caller, pid, tid, linux(sig)).await?;
        Ok(())
    })
    .await
}

/// `proc_signal(pid, sig)`: `kill`.
pub(crate) async fn proc_signal<S: Syscalls + Control + 'static>(
    env: Env<S>,
    pid: i32,
    sig: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        frame
            .syscalls
            .kill(&frame.caller(), pid, linux(sig))
            .await?;
        Ok(())
    })
    .await
}

/// The `ITIMER_REAL` setting for `proc_raise_interval(SIGALRM, interval, repeat)`: the first
/// signal after `interval` nanoseconds, then every `interval` if `repeat`; 0 disarms.
fn alarm_timer(interval: u64, repeat: bool) -> KernelOldItimerval {
    // Round up, so that a short interval does not disarm the timer.
    let micros = interval.div_ceil(1_000);
    let value = KernelOldTimeval {
        tv_sec: (micros / 1_000_000).min(i32::MAX as u64) as i32,
        tv_usec: (micros % 1_000_000) as i32,
    };
    KernelOldItimerval {
        it_interval: if repeat {
            value
        } else {
            KernelOldTimeval::ZERO
        },
        it_value: value,
    }
}

/// `proc_raise_interval(sig, interval, repeat)`: a timer that raises `sig`. Linux's
/// `ITIMER_REAL` serves `SIGALRM`, which is all wasix-libc asks for (`setitimer`); other
/// signals would need POSIX timers, which the kernel does not have.
pub(crate) async fn proc_raise_interval<S: Syscalls + Control + 'static>(
    env: Env<S>,
    sig: i32,
    interval: i64,
    repeat: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        if sig != i32::from(SIGALRM) {
            return shim::fail(Errno::NOTSUP);
        }
        let timer = alarm_timer(interval as u64, repeat as u8 == Bool::TRUE.0);
        let timer = frame.mem.alloc(&timer)?;
        frame
            .syscalls
            .setitimer(
                &frame.caller(),
                ItimerWhich::ITIMER_REAL,
                timer,
                KernelOldItimervalPtr::NULL,
            )
            .await?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_signals() {
        assert!(delivered(2) && delivered(14) && delivered(10) && delivered(64));
        for sig in [SIGKILL, SIGSTOP, 17, 18, 23, 28] {
            assert!(!delivered(sig), "{sig}");
        }
    }

    #[test]
    fn blocked_mask_spares_kill_and_stop() {
        let mask = all_blocked();
        let bits = u64::from(mask.sig[0]) | u64::from(mask.sig[1]) << 32;
        assert_eq!(bits & (1 << 8), 0);
        assert_eq!(bits & (1 << 18), 0);
        assert_eq!(bits.count_ones(), 62);
    }

    #[test]
    fn alarm_timers() {
        let once = alarm_timer(1_500_000_000, false);
        assert_eq!((once.it_value.tv_sec, once.it_value.tv_usec), (1, 500_000));
        assert_eq!(once.it_interval, KernelOldTimeval::ZERO);
        let repeating = alarm_timer(1, true);
        assert_eq!(repeating.it_value.tv_usec, 1);
        assert_eq!(repeating.it_interval, repeating.it_value);
        assert_eq!(alarm_timer(0, true), KernelOldItimerval::ZERO);
    }
}
