//! Integer sysctls kept in one table (`kernel/sysctl.c`, `proc_dointvec_minmax`): each has its
//! default, its range and what it does here; procfs places them under `/proc/sys`.
//!
//! Some are acted on: `kernel.threads-max` bounds the tasks `fork` makes, and
//! `kernel.sched_rr_timeslice_ms` is the `SCHED_RR` quantum, `fs.inotify.max_queued_events` the
//! queue of a new inotify instance. The rest tune subsystems the machine does not have (page
//! cache writeback, swap, the OOM killer, real-time throttling, panics): they keep what is
//! written, within Linux's range, and change nothing, as the scheduling attributes do.

use std::sync::atomic::{AtomicI64, Ordering};

/// `MAX_THREADS` (`FUTEX_TID_MASK`) and `MIN_THREADS`.
const MAX_THREADS: i64 = 0x3fff_ffff;
const MIN_THREADS: i64 = 20;
/// `THREAD_SIZE` of a 32-bit kernel, which `set_max_threads` divides memory by.
const THREAD_SIZE: u64 = 8 * 1024;

/// How a write is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Write {
    /// `proc_dointvec_minmax`: a value in range, else `EINVAL`.
    Range(i64, i64),
    /// `proc_do_rointvec`: accepted and ignored.
    Ignored,
}

/// An integer sysctl.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tunable {
    ThreadsMax,
    Sysrq,
    Panic,
    PanicOnOops,
    SchedChildRunsFirst,
    SchedRrTimesliceMs,
    SchedRtPeriodUs,
    SchedRtRuntimeUs,
    RandomWriteWakeupThreshold,
    RandomUrandomMinReseedSecs,
    InotifyMaxQueuedEvents,
    Swappiness,
    DirtyRatio,
    DirtyBackgroundRatio,
    DirtyExpireCentisecs,
    DirtyWritebackCentisecs,
    VfsCachePressure,
    MinFreeKbytes,
    PanicOnOom,
    OomKillAllocatingTask,
    OomDumpTasks,
    PageCluster,
    StatInterval,
    LaptopMode,
    AdminReserveKbytes,
    UserReserveKbytes,
}

impl Tunable {
    pub(crate) const ALL: [Tunable; 26] = [
        Tunable::ThreadsMax,
        Tunable::Sysrq,
        Tunable::Panic,
        Tunable::PanicOnOops,
        Tunable::SchedChildRunsFirst,
        Tunable::SchedRrTimesliceMs,
        Tunable::SchedRtPeriodUs,
        Tunable::SchedRtRuntimeUs,
        Tunable::RandomWriteWakeupThreshold,
        Tunable::RandomUrandomMinReseedSecs,
        Tunable::InotifyMaxQueuedEvents,
        Tunable::Swappiness,
        Tunable::DirtyRatio,
        Tunable::DirtyBackgroundRatio,
        Tunable::DirtyExpireCentisecs,
        Tunable::DirtyWritebackCentisecs,
        Tunable::VfsCachePressure,
        Tunable::MinFreeKbytes,
        Tunable::PanicOnOom,
        Tunable::OomKillAllocatingTask,
        Tunable::OomDumpTasks,
        Tunable::PageCluster,
        Tunable::StatInterval,
        Tunable::LaptopMode,
        Tunable::AdminReserveKbytes,
        Tunable::UserReserveKbytes,
    ];

    /// Linux's default (`init/Kconfig`, the subsystems' initialisers).
    fn default(self) -> i64 {
        match self {
            Tunable::ThreadsMax => {
                // `set_max_threads`: memory over eight thread stacks per thread.
                let threads = crate::resource::ADDRESS_SPACE / (THREAD_SIZE * 8);
                (threads as i64).clamp(MIN_THREADS, MAX_THREADS)
            }
            // `CONFIG_MAGIC_SYSRQ_DEFAULT_ENABLE`.
            Tunable::Sysrq => 1,
            Tunable::Panic | Tunable::PanicOnOops => 0,
            Tunable::SchedChildRunsFirst => 0,
            Tunable::SchedRrTimesliceMs => 100,
            Tunable::SchedRtPeriodUs => 1_000_000,
            Tunable::SchedRtRuntimeUs => 950_000,
            Tunable::RandomWriteWakeupThreshold => 256,
            Tunable::RandomUrandomMinReseedSecs => 60,
            Tunable::InotifyMaxQueuedEvents => 16384,
            Tunable::Swappiness => 60,
            Tunable::DirtyRatio => 20,
            Tunable::DirtyBackgroundRatio => 10,
            Tunable::DirtyExpireCentisecs => 3000,
            Tunable::DirtyWritebackCentisecs => 500,
            Tunable::VfsCachePressure => 100,
            Tunable::MinFreeKbytes => 1024,
            Tunable::PanicOnOom | Tunable::OomKillAllocatingTask => 0,
            Tunable::OomDumpTasks => 1,
            Tunable::PageCluster => 3,
            Tunable::StatInterval => 1,
            Tunable::LaptopMode => 0,
            Tunable::AdminReserveKbytes => 8192,
            Tunable::UserReserveKbytes => 131072,
        }
    }

    /// How a write is taken.
    pub(crate) fn write(self) -> Write {
        let int = i64::from(i32::MAX);
        match self {
            Tunable::ThreadsMax => Write::Range(MIN_THREADS, MAX_THREADS),
            Tunable::Sysrq | Tunable::Panic | Tunable::PanicOnOops => {
                Write::Range(i64::from(i32::MIN), int)
            }
            Tunable::SchedChildRunsFirst | Tunable::OomKillAllocatingTask => Write::Range(0, int),
            Tunable::SchedRrTimesliceMs => Write::Range(i64::from(i32::MIN), int),
            Tunable::SchedRtPeriodUs => Write::Range(1, int),
            Tunable::SchedRtRuntimeUs => Write::Range(-1, int),
            Tunable::RandomWriteWakeupThreshold | Tunable::RandomUrandomMinReseedSecs => {
                Write::Ignored
            }
            Tunable::InotifyMaxQueuedEvents => Write::Range(0, int),
            Tunable::Swappiness => Write::Range(0, 200),
            Tunable::DirtyRatio | Tunable::DirtyBackgroundRatio => Write::Range(0, 100),
            Tunable::DirtyExpireCentisecs | Tunable::DirtyWritebackCentisecs => {
                Write::Range(0, int)
            }
            Tunable::VfsCachePressure | Tunable::MinFreeKbytes => Write::Range(0, int),
            Tunable::PanicOnOom => Write::Range(0, 2),
            Tunable::OomDumpTasks | Tunable::LaptopMode | Tunable::StatInterval => {
                Write::Range(0, int)
            }
            Tunable::PageCluster => Write::Range(0, 31),
            Tunable::AdminReserveKbytes | Tunable::UserReserveKbytes => {
                Write::Range(0, i64::from(u32::MAX))
            }
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|&t| t == self)
            .expect("every tunable is listed")
    }
}

/// The values of the [`Tunable`]s.
pub(crate) struct Tunables([AtomicI64; Tunable::ALL.len()]);

impl Default for Tunables {
    fn default() -> Self {
        Self(Tunable::ALL.map(|t| AtomicI64::new(t.default())))
    }
}

impl Tunables {
    pub(crate) fn get(&self, tunable: Tunable) -> i64 {
        self.0[tunable.index()].load(Ordering::Relaxed)
    }

    /// Writes `value`, as `tunable` takes it.
    pub(crate) fn set(&self, tunable: Tunable, value: i64) -> Result<(), wasixv2_syscalls::Errno> {
        // `sched_rr_handler`: none or less is the default quantum.
        let value = match tunable {
            Tunable::SchedRrTimesliceMs if value <= 0 => tunable.default(),
            _ => value,
        };
        match tunable.write() {
            Write::Ignored => Ok(()),
            Write::Range(min, max) if (min..=max).contains(&value) => {
                self.0[tunable.index()].store(value, Ordering::Relaxed);
                Ok(())
            }
            Write::Range(..) => Err(wasixv2_syscalls::Errno::EINVAL),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunables_keep_values_in_range() {
        let tunables = Tunables::default();
        assert_eq!(tunables.get(Tunable::ThreadsMax), 65536);
        assert_eq!(tunables.set(Tunable::Swappiness, 10), Ok(()));
        assert_eq!(tunables.get(Tunable::Swappiness), 10);
        assert!(tunables.set(Tunable::Swappiness, 201).is_err());
        assert!(tunables.set(Tunable::ThreadsMax, 19).is_err());
        assert_eq!(tunables.set(Tunable::RandomWriteWakeupThreshold, 7), Ok(()));
        assert_eq!(tunables.get(Tunable::RandomWriteWakeupThreshold), 256);
    }
}
