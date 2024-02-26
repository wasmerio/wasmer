#[cfg(feature = "journal")]
mod actions;
mod clear_ethereal;
mod do_checkpoint_from_outside;
mod maybe_snapshot;
mod maybe_snapshot_many;
mod maybe_snapshot_once;
#[cfg(feature = "journal")]
mod play_event;
mod restore_snapshot;
mod wait_for_snapshot;

#[cfg(feature = "journal")]
use actions::*;
use clear_ethereal::*;
use wasmer_journal::JournalEntry;

pub use do_checkpoint_from_outside::*;
pub use maybe_snapshot::*;
pub use maybe_snapshot_many::*;
pub use maybe_snapshot_once::*;
pub use restore_snapshot::*;
pub use wait_for_snapshot::*;

use crate::os::task::process::MemorySnapshotRegion;
use std::{collections::BTreeMap, ops::Range};

use super::*;

pub struct JournalSyscallPlayer<'a, 'c> {
    pub ctx: FunctionEnvMut<'c, WasiEnv>,
    pub bootstrapping: bool,

    pub journal_module_hash: Option<[u8; 8]>,
    pub rewind: Option<RewindState>,
    pub cur_module_hash: [u8; 8],
    pub real_fd: HashSet<WasiFd>,

    // We delay the spawning of threads until the end as its
    // possible that the threads will be cancelled before all the
    // events finished the streaming process
    pub spawn_threads: BTreeMap<WasiThreadId, RewindState>,
    pub staged_differ_memory: Vec<(Range<u64>, Cow<'a, [u8]>)>,
    pub differ_memory: Vec<(Range<u64>, Cow<'a, [u8]>)>,

    // We capture the stdout and stderr while we replay
    pub stdout: Vec<(u64, Cow<'a, [u8]>, bool)>,
    pub stderr: Vec<(u64, Cow<'a, [u8]>, bool)>,
    pub stdout_fds: HashSet<u32>,
    pub stderr_fds: HashSet<u32>,
}

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    pub fn new(mut ctx: FunctionEnvMut<'c, WasiEnv>, bootstrapping: bool) -> Self {
        let cur_module_hash: [u8; 8] = ctx.data().process.module_hash.as_bytes();
        let mut ret = JournalSyscallPlayer {
            ctx,
            bootstrapping,
            cur_module_hash,
            journal_module_hash: None,
            rewind: None,
            spawn_threads: Default::default(),
            staged_differ_memory: Default::default(),
            differ_memory: Default::default(),
            stdout: Default::default(),
            stderr: Default::default(),
            stdout_fds: Default::default(),
            stderr_fds: Default::default(),
            real_fd: Default::default(),
        };

        // We capture the stdout and stderr while we replay
        ret.stdout_fds.insert(1 as WasiFd);
        ret.stderr_fds.insert(2 as WasiFd);

        ret
    }
}
