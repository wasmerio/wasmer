//! BSD process accounting (`kernel/acct.c`, `CONFIG_BSD_PROCESS_ACCT_V3`): `acct(2)` names a
//! file per pid namespace, and every process that exits in it or below appends an `acct_v3`
//! record there, in each namespace up from its own that has one (`slow_acct_process`).
//!
//! A record is made when the process's last task is done (`acct_process` in `do_exit`), and
//! written by the next call that may wait: the exit's own `exit_mm`, or a `waitid` before it
//! reports the process, so its parent never sees it gone before its record. The file is
//! written with the credentials it was opened with (`f_cred`), with no regard to
//! `RLIMIT_FSIZE`. The free space of its filesystem is not watched: accounting never pauses.

use std::{
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use wasixv2_abi::{Cstr, OpenFlags};
use wasixv2_syscalls::{Caller, Errno, SysResult};
use wasixv2_vfs::{Cap, File, FileType, LookupFlags, OpenHow};

use crate::{Kernel, Process, ns::PidNs, ns::UserNs, sync::MutexExt};

/// `ACCT_VERSION` 3, little-endian (`ACCT_BYTEORDER` 0).
const ACCT_VERSION: u8 = 3;
/// `AHZ`: the clock of version 3's times.
const AHZ: u64 = 100;
/// `ACCT_COMM`.
const ACCT_COMM: usize = 16;
/// `sizeof(struct acct_v3)`.
pub(crate) const ACCT_V3_SIZE: usize = 64;

/// The `ac_flag` bits.
pub(crate) const AFORK: u8 = 0x01;
pub(crate) const ACORE: u8 = 0x08;
pub(crate) const AXSIG: u8 = 0x10;

/// An accounting file (`struct bsd_acct_struct`).
struct AcctFile {
    file: File,
    /// The user namespace of the credentials it was opened with, which the ids are written
    /// as (`file->f_cred->user_ns`).
    user_ns: Arc<UserNs>,
    /// The pid namespace it accounts for, whose pids it is written with.
    ns: Weak<PidNs>,
}

/// The accounting files of every pid namespace that has one (`ns->bacct`), and the records
/// made but not yet written.
#[derive(Default)]
pub(crate) struct Acct {
    files: Mutex<Vec<Arc<AcctFile>>>,
    pending: Mutex<Vec<(Arc<AcctFile>, [u8; ACCT_V3_SIZE])>>,
}

impl Acct {
    /// The file of the pid namespace `ns`.
    fn file_of(&self, ns: &Arc<PidNs>) -> Option<Arc<AcctFile>> {
        let mut files = self.files.lock_or_recover();
        files.retain(|file| file.ns.strong_count() > 0);
        files
            .iter()
            .find(|file| file.ns.upgrade().is_some_and(|at| Arc::ptr_eq(&at, ns)))
            .cloned()
    }

    /// `ns->bacct = new`: the namespace's file is `new`, or none.
    fn set(&self, ns: &Arc<PidNs>, new: Option<AcctFile>) {
        let mut files = self.files.lock_or_recover();
        files.retain(|file| file.ns.upgrade().is_some_and(|at| !Arc::ptr_eq(&at, ns)));
        files.extend(new.map(Arc::new));
    }
}

/// What a process's exit tells its record (`struct pacct_struct`).
pub(crate) struct Pacct {
    /// The leader's exit code as `wait` reports it.
    pub(crate) exit_code: u32,
    pub(crate) flags: u8,
    /// The size of its address space, in KiB.
    pub(crate) mem_kb: u64,
    pub(crate) comm: [u8; ACCT_COMM],
    /// Its controlling terminal (`old_encode_dev`), 0 for none.
    pub(crate) tty: u16,
    /// Its parent's id, 0 for none.
    pub(crate) parent: u32,
}

/// `encode_comp_t`: a 13-bit mantissa with a 3-bit base-8 exponent.
fn encode_comp_t(mut value: u64) -> u16 {
    const MANTSIZE: u32 = 13;
    const EXPSIZE: u32 = 3;
    const MAXFRACT: u64 = (1 << MANTSIZE) - 1;
    let (mut exp, mut rnd) = (0u32, 0);
    while value > MAXFRACT {
        rnd = value & (1 << (EXPSIZE - 1));
        value >>= EXPSIZE;
        exp += 1;
    }
    if rnd != 0 {
        value += 1;
        if value > MAXFRACT {
            value >>= EXPSIZE;
            exp += 1;
        }
    }
    if exp > u32::from(u16::MAX) >> MANTSIZE {
        return u16::MAX;
    }
    ((exp << MANTSIZE) as u64 + value) as u16
}

/// `encode_float`: a 32-bit IEEE float of `value`.
fn encode_float(mut value: u64) -> u32 {
    if value == 0 {
        return 0;
    }
    let mut exp = 190u32;
    while (value as i64) > 0 {
        value <<= 1;
        exp -= 1;
    }
    ((value >> 40) as u32 & 0x7f_ffff) | (exp << 23)
}

fn ahz(d: Duration) -> u64 {
    (d.as_nanos() / (1_000_000_000 / u128::from(AHZ))) as u64
}

impl Kernel {
    /// `acct(2)`: with `CAP_SYS_PACCT`, starts accounting the caller's pid namespace to the
    /// file `name` (`acct_on`), a regular file outside procfs and sysfs opened for appending,
    /// or with a null `name` stops it.
    pub(crate) async fn sys_acct(&self, caller: &Caller<'_>, name: Cstr) -> SysResult<i32> {
        let cred = self.cred(caller);
        if !cred.capable_init(Cap::SysPacct) {
            return Err(Errno::EPERM.into());
        }
        let ns = self.active_pid_ns(caller.task());
        if name.is_null() {
            self.acct.set(&ns, None);
            return Ok(0);
        }
        let path = crate::fs::read_path(caller.memory(), name).await?;
        let env = self.env(caller);
        let how = OpenHow {
            flags: OpenFlags::O_WRONLY | OpenFlags::O_APPEND | OpenFlags::O_LARGEFILE,
            mode: 0,
            lookup: LookupFlags::follow(),
        };
        let file = self
            .vfs
            .open(&env.cx(), &env.fs.cwd, &path, &how)
            .await
            .map_err(|errno| env.error(errno))?;
        if file.file_type() != FileType::Regular {
            return Err(Errno::EACCES.into());
        }
        let magic = file.path.dentry.sb().info.magic;
        if magic == crate::procfs::PROC_SUPER_MAGIC || magic == crate::sysfs::SYSFS_MAGIC {
            return Err(Errno::EINVAL.into());
        }
        let new = AcctFile {
            file,
            user_ns: cred.user_ns.clone(),
            ns: Arc::downgrade(&ns),
        };
        self.acct.set(&ns, Some(new));
        Ok(0)
    }

    /// `acct_process` for `process`, whose last task is done: a record for each pid namespace
    /// from its own up that accounts, to be written by [`Kernel::acct_flush`].
    pub(crate) fn acct_process(&self, process: &Process, pacct: Pacct) {
        let mut at = Some(process.pid_ns.clone());
        while let Some(ns) = at {
            if let Some(file) = self.acct.file_of(&ns) {
                let record = self.acct_record(process, &pacct, &file);
                self.acct.pending.lock_or_recover().push((file, record));
            }
            at = ns.parent().cloned();
        }
    }

    /// `fill_ac`: the `acct_v3` record of `process` for `file`.
    fn acct_record(&self, process: &Process, pacct: &Pacct, file: &AcctFile) -> [u8; ACCT_V3_SIZE] {
        let elapsed = self.host.monotonic().saturating_sub(process.start);
        let btime = self.host.realtime().saturating_sub(elapsed).as_secs();
        let times = process.cpu.times();
        let cred = self.task_cred(process.pid);
        let (pid, ppid) = match file.ns.upgrade() {
            Some(ns) => (
                self.pids.nr(&ns, process.pid.0),
                match pacct.parent {
                    0 => 0,
                    parent => self.pids.nr(&ns, parent),
                },
            ),
            None => (0, 0),
        };
        let mut out = [0u8; ACCT_V3_SIZE];
        out[0] = pacct.flags;
        out[1] = ACCT_VERSION;
        out[2..4].copy_from_slice(&pacct.tty.to_le_bytes());
        let words = [
            pacct.exit_code,
            file.user_ns.munged_uid(cred.user.real),
            file.user_ns.munged_gid(cred.group.real),
            pid as u32,
            ppid as u32,
            u32::try_from(btime).unwrap_or(u32::MAX),
            encode_float(ahz(elapsed)),
        ];
        for (i, word) in words.iter().enumerate() {
            out[4 + 4 * i..8 + 4 * i].copy_from_slice(&word.to_le_bytes());
        }
        let comps = [
            encode_comp_t(ahz(times.user)),
            encode_comp_t(ahz(times.system)),
            encode_comp_t(pacct.mem_kb),
            0,
            0,
            0,
            0,
            0,
        ];
        for (i, comp) in comps.iter().enumerate() {
            out[32 + 2 * i..34 + 2 * i].copy_from_slice(&comp.to_le_bytes());
        }
        out[48..64].copy_from_slice(&pacct.comm);
        out
    }

    /// Writes the records made so far (`acct_write_process`), in order.
    pub(crate) async fn acct_flush(&self) {
        loop {
            let pending = std::mem::take(&mut *self.acct.pending.lock_or_recover());
            if pending.is_empty() {
                return;
            }
            for (file, record) in pending {
                // A failed write loses the record, as `__kernel_write`'s result is ignored.
                let _ = file.file.append(&record).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comp_t_and_float_encode_as_linux_does() {
        assert_eq!(encode_comp_t(0), 0);
        assert_eq!(encode_comp_t(8191), 8191);
        // 8192 = 1024 << 3: exponent 1, mantissa 1024.
        assert_eq!(encode_comp_t(8192), (1 << 13) | 1024);
        assert_eq!(encode_comp_t(u64::MAX), u16::MAX);
        assert_eq!(encode_float(0), 0);
        assert_eq!(f32::from_bits(encode_float(1)), 1.0);
        assert_eq!(f32::from_bits(encode_float(150)), 150.0);
    }
}
