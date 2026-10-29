//! `reboot` (`kernel/reboot.c`, `reboot_pid_ns` in `kernel/pid_namespace.c`).
//!
//! The machine has no hardware to restart or power off: in the initial pid namespace, a
//! restart, halt or power-off ends every process of the machine, as [`Kernel::terminate`]
//! does, and leaves the request for the embedder ([`Kernel::power_request`]). In a child pid
//! namespace, as on Linux, it kills the namespace's init, which its parent then sees killed by
//! `SIGHUP` (restart) or `SIGINT` (halt, power-off).

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use wasixv2_abi::{RebootCmd, RebootMagic, Signal, VoidPtr};
use wasixv2_syscalls::{Caller, Errno, SysResult};
use wasixv2_vfs::Cap;

use crate::{Kernel, sync::MutexExt};

/// What a `reboot` in the initial pid namespace asked the machine to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerRequest {
    /// `LINUX_REBOOT_CMD_RESTART` and `RESTART2`.
    Restart,
    /// `LINUX_REBOOT_CMD_HALT`.
    Halt,
    /// `LINUX_REBOOT_CMD_POWER_OFF`.
    PowerOff,
}

/// The kernel's reboot state.
pub(crate) struct Reboot {
    /// `C_A_D`: Ctrl-Alt-Del reboots at once rather than signalling init. On by default.
    ctrl_alt_del: AtomicBool,
    /// The last restart, halt or power-off asked for in the initial pid namespace.
    request: Mutex<Option<PowerRequest>>,
}

impl Default for Reboot {
    fn default() -> Self {
        Self {
            ctrl_alt_del: AtomicBool::new(true),
            request: Mutex::new(None),
        }
    }
}

/// Whether `magic1` and `magic2` are `reboot`'s magic numbers.
fn magic_ok(magic1: i32, magic2: i32) -> bool {
    let magic2 = magic2 as u32;
    magic1 as u32 == RebootMagic::LINUX_REBOOT_MAGIC1.0
        && [
            RebootMagic::LINUX_REBOOT_MAGIC2,
            RebootMagic::LINUX_REBOOT_MAGIC2A,
            RebootMagic::LINUX_REBOOT_MAGIC2B,
            RebootMagic::LINUX_REBOOT_MAGIC2C,
        ]
        .iter()
        .any(|m| m.0 == magic2)
}

impl Kernel {
    /// `sys_reboot`. `CAP_SYS_BOOT` first, then the magic numbers, then the command. A child
    /// pid namespace takes the commands that end it (`reboot_pid_ns`); `LINUX_REBOOT_CMD_KEXEC`
    /// and `SW_SUSPEND` are `EINVAL`, as on a kernel without kexec and hibernation.
    pub(crate) fn sys_reboot(
        &self,
        caller: &Caller<'_>,
        magic1: i32,
        magic2: i32,
        cmd: RebootCmd,
        _arg: VoidPtr,
    ) -> SysResult<i32> {
        if !self.cred(caller).capable(Cap::SysBoot) {
            return Err(Errno::EPERM.into());
        }
        if !magic_ok(magic1, magic2) {
            return Err(Errno::EINVAL.into());
        }
        let ending = match cmd {
            RebootCmd::LINUX_REBOOT_CMD_RESTART | RebootCmd::LINUX_REBOOT_CMD_RESTART2 => {
                Some(PowerRequest::Restart)
            }
            RebootCmd::LINUX_REBOOT_CMD_HALT => Some(PowerRequest::Halt),
            RebootCmd::LINUX_REBOOT_CMD_POWER_OFF => Some(PowerRequest::PowerOff),
            _ => None,
        };
        let pid_ns = self.process(caller.task()).pid_ns.clone();
        if pid_ns.level() > 0 {
            // `reboot_pid_ns`: the exit signal of the namespace's init, which is sent SIGKILL;
            // the caller goes with the namespace.
            let sig = match ending.ok_or(Errno::EINVAL)? {
                PowerRequest::Restart => Signal::SIGHUP,
                PowerRequest::Halt | PowerRequest::PowerOff => Signal::SIGINT,
            };
            pid_ns.set_reboot(sig.0);
            if let Some(signals) = pid_ns
                .reaper()
                .and_then(|init| self.jobs.group_signals(init))
            {
                signals.send_kernel(Signal::SIGKILL.0);
            }
            return Ok(0);
        }
        match cmd {
            RebootCmd::LINUX_REBOOT_CMD_CAD_ON => {
                self.reboot.ctrl_alt_del.store(true, Ordering::Relaxed);
            }
            RebootCmd::LINUX_REBOOT_CMD_CAD_OFF => {
                self.reboot.ctrl_alt_del.store(false, Ordering::Relaxed);
            }
            _ => {
                let request = ending.ok_or(Errno::EINVAL)?;
                // `kernel_restart`, `kernel_halt`, `kernel_power_off`: the machine stops, and
                // the caller with it, at its return to the guest.
                *self.reboot.request.lock_or_recover() = Some(request);
                self.kill_everything();
            }
        }
        Ok(0)
    }

    /// The restart, halt or power-off a process asked for with `reboot` in the initial pid
    /// namespace, after which every process of the machine was killed; `None` if none did.
    pub fn power_request(&self) -> Option<PowerRequest> {
        *self.reboot.request.lock_or_recover()
    }

    /// Whether Ctrl-Alt-Del reboots at once (`C_A_D`, `LINUX_REBOOT_CMD_CAD_ON`).
    pub fn ctrl_alt_del(&self) -> bool {
        self.reboot.ctrl_alt_del.load(Ordering::Relaxed)
    }
}
