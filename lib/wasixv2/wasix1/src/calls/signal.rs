//! Signals.
//!
//! wasix-libc keeps its own disposition table. At startup it asks for the dispositions the
//! process inherited (`proc_signals_*`, failing hard if that fails) and names its handler
//! export (`callback_signal`).

use wasixv2_abi::{ConstSigactionPtr, GuestLayout, Sigaction, SignalHandler, Sigset};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::proc::{Disposition, Signal, SignalDisposition},
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// The signals the process inherited as ignored, which is everything v1 reports: the rest are
/// at their default action.
async fn inherited<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
) -> V1Result<Vec<SignalDisposition>> {
    let mut ignored = Vec::new();
    for sig in 1..=31 {
        let Some(signal) = Signal::from_linux(sig) else {
            continue;
        };
        if signal == Signal::SIGKILL || signal == Signal::SIGSTOP {
            continue;
        }
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
        if frame.mem.take(old).sa_handler == SignalHandler::SIG_IGN.0 as u32 {
            ignored.push(SignalDisposition {
                sig: signal,
                disp: Disposition::IGNORE,
            });
        }
    }
    Ok(ignored)
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

/// `callback_signal(name, name_len)`: libc names its handler export at startup.
///
/// Placeholder until signal delivery to v1 guests exists (DESIGN.md, "WASIX v1
/// compatibility"): nothing is registered, so signals take their default actions, as under v1
/// before the first registration.
pub(crate) async fn callback_signal<S: Syscalls + Control + 'static>(
    _env: Env<S>,
    _name: i32,
    _name_len: i32,
) -> Result<(), RuntimeError> {
    Ok(())
}
