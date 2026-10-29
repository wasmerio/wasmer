//! IPC namespace limits (`ipc/ipc_sysctl.c:ipc_sysctls`). Syscalls and procfs use
//! the same values, including after a namespace's limits are lowered below its usage.

use wasixv2_syscalls::Errno;

use super::{msg, sem, shm};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sysctl {
    ShmMax,
    ShmAll,
    ShmMni,
    MsgMax,
    MsgMni,
    MsgMnb,
    Sem,
}

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub(crate) shmmax: u32,
    pub(crate) shmall: u32,
    pub(crate) shmmni: u32,
    pub(crate) msgmax: u32,
    pub(crate) msgmni: u32,
    pub(crate) msgmnb: u32,
    /// SEMMSL, SEMMNS, SEMOPM, SEMMNI; `proc_dointvec` accepts signed ints.
    pub(crate) sem: [i32; 4],
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            shmmax: shm::SHMMAX,
            shmall: shm::SHMALL,
            shmmni: shm::SHMMNI,
            msgmax: msg::MSGMAX as u32,
            msgmni: msg::MSGMNI as u32,
            msgmnb: msg::MSGMNB as u32,
            sem: [sem::SEMMSL, sem::SEMMNS, sem::SEMOPM as i32, sem::SEMMNI],
        }
    }
}

impl Limits {
    pub(crate) fn text(&self, which: Sysctl) -> String {
        match which {
            Sysctl::ShmMax => self.shmmax.to_string(),
            Sysctl::ShmAll => self.shmall.to_string(),
            Sysctl::ShmMni => self.shmmni.to_string(),
            Sysctl::MsgMax => self.msgmax.to_string(),
            Sysctl::MsgMni => self.msgmni.to_string(),
            Sysctl::MsgMnb => self.msgmnb.to_string(),
            Sysctl::Sem => self
                .sem
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join("\t"),
        }
    }

    pub(crate) fn write(&mut self, which: Sysctl, data: &[u8]) -> Result<(), Errno> {
        let text = std::str::from_utf8(data).map_err(|_| Errno::EINVAL)?;
        let mut words = text.split_whitespace().peekable();
        if words.peek().is_none() {
            return Err(Errno::EINVAL);
        }
        if which == Sysctl::Sem {
            // ipc/ipc_sysctl.c:proc_ipc_sem_dointvec restores SEMMNI on error;
            // updates to the earlier words remain, as with proc_dointvec.
            let old_mni = self.sem[3];
            for (at, word) in words.take(4).enumerate() {
                self.sem[at] = word.parse().map_err(|_| Errno::EINVAL)?;
            }
            if !(0..=super::IPCMNI as i32).contains(&self.sem[3]) {
                self.sem[3] = old_mni;
                return Err(Errno::EINVAL);
            }
            return Ok(());
        }
        let word = words.next().ok_or(Errno::EINVAL)?;
        let unsigned = matches!(which, Sysctl::ShmMax | Sysctl::ShmAll);
        // proc_doulongvec_minmax consumes negative values without changing the limit.
        if unsigned && word.starts_with('-') {
            let _: i64 = word.parse().map_err(|_| Errno::EINVAL)?;
            return Ok(());
        }
        let value: u32 = word.parse().map_err(|_| Errno::EINVAL)?;
        let max = match which {
            Sysctl::ShmMax | Sysctl::ShmAll => u32::MAX,
            Sysctl::ShmMni | Sysctl::MsgMni => super::IPCMNI as u32,
            _ => i32::MAX as u32,
        };
        if value > max {
            return Err(Errno::EINVAL);
        }
        *match which {
            Sysctl::ShmMax => &mut self.shmmax,
            Sysctl::ShmAll => &mut self.shmall,
            Sysctl::ShmMni => &mut self.shmmni,
            Sysctl::MsgMax => &mut self.msgmax,
            Sysctl::MsgMni => &mut self.msgmni,
            Sysctl::MsgMnb => &mut self.msgmnb,
            Sysctl::Sem => unreachable!(),
        } = value;
        Ok(())
    }
}
