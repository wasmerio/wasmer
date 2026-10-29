//! The `/proc/<pid>` files that look into a process's scheduling, memory, I/O, OOM score and
//! audit identity (fs/proc/base.c, fs/proc/task_mmu.c, kernel/sched/debug.c): what tools such
//! as ps, top, htop, systemd and the OOM tuners read.

use std::{fmt::Write as _, sync::Arc};

use wasixv2_vfs::{MntNs, Path};

use super::{Source, TaskInfo, text::escape};
use crate::{
    mm::{Mm, PAGE_SIZE, RegionKind},
    ns::UserNs,
    sync::MutexExt as _,
};

/// `OOM_SCORE_ADJ_MIN` and `OOM_SCORE_ADJ_MAX`.
pub(super) const OOM_SCORE_ADJ_MIN: i32 = -1000;
pub(super) const OOM_SCORE_ADJ_MAX: i32 = 1000;
/// `OOM_DISABLE` and `OOM_ADJUST_MAX`, the legacy `oom_adj` range.
pub(super) const OOM_DISABLE: i32 = -17;
pub(super) const OOM_ADJUST_MAX: i32 = 15;

/// `proc_oom_score`: `1000 + badness * 1000 / totalpages`, where the badness is the pages the
/// process holds plus its `oom_score_adj` scaled to the machine; 0 for a process the OOM killer
/// never picks (the init, `oom_score_adj` -1000).
pub(super) fn oom_score(process: &crate::Process, task: &TaskInfo, mm: Option<&Mm>) -> Vec<u8> {
    let adj = process.ident.lock_or_recover().oom_score_adj;
    let unkillable = task.signals.group().unkillable() || adj == OOM_SCORE_ADJ_MIN;
    let points = match unkillable {
        true => 0,
        false => {
            let total = crate::resource::ADDRESS_SPACE;
            let rss = mm.map_or(0, Mm::size);
            let badness = (rss * 1000 / total) as i64 + i64::from(adj);
            (1000 + badness).clamp(0, 2000)
        }
    };
    format!("{points}\n").into_bytes()
}

/// `oom_adj_read`: `oom_score_adj` on the legacy scale.
pub(super) fn oom_adj(adj: i32) -> i32 {
    match adj {
        OOM_SCORE_ADJ_MAX => OOM_ADJUST_MAX,
        adj => adj * -OOM_DISABLE / OOM_SCORE_ADJ_MAX,
    }
}

/// `proc_pid_schedstat`: time on the CPU and waiting for it, in nanoseconds, and timeslices.
/// The host schedules: no wait is seen, and each run is one slice.
pub(super) fn schedstat(task: &TaskInfo) -> Vec<u8> {
    let run = (task.cpu.user + task.cpu.system).as_nanos();
    format!("{run} 0 0\n").into_bytes()
}

/// `proc_sched_show_task` (`/proc/<pid>/sched`), the fields this kernel has.
pub(super) fn sched(comm: &[u8], pid: i32, threads: usize, task: &TaskInfo) -> Vec<u8> {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} ({pid}, #threads: {threads})",
        String::from_utf8_lossy(comm)
    );
    let _ = writeln!(out, "{}", "-".repeat(67));
    let run = (task.cpu.user + task.cpu.system).as_nanos();
    let ms = |ns: u128| format!("{:14}.{:06}", ns / 1_000_000, ns % 1_000_000);
    let lines: [(&str, String); 11] = [
        ("se.exec_start", ms(0)),
        ("se.vruntime", ms(run)),
        ("se.sum_exec_runtime", ms(run)),
        ("se.nr_migrations", format!("{:21}", 0)),
        ("nr_switches", format!("{:21}", 0)),
        ("nr_voluntary_switches", format!("{:21}", 0)),
        ("nr_involuntary_switches", format!("{:21}", 0)),
        (
            "se.load.weight",
            format!("{:21}", nice_weight(task.sched.nice()) << 10),
        ),
        ("policy", format!("{:21}", task.sched.policy().0)),
        ("prio", format!("{:21}", task.sched.prio())),
        ("clock-delta", format!("{:21}", 0)),
    ];
    for (name, value) in lines {
        let _ = writeln!(out, "{name:<45}:{value}");
    }
    out.into_bytes()
}

/// `sched_prio_to_weight`: the load weight of a nice value.
fn nice_weight(nice: i32) -> u64 {
    const WEIGHTS: [u64; 40] = [
        88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705, 14949, 11916, 9548, 7620, 6100,
        4904, 3906, 3121, 2501, 1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215, 172,
        137, 110, 87, 70, 56, 45, 36, 29, 23, 18, 15,
    ];
    WEIGHTS[(nice + 20).clamp(0, 39) as usize]
}

/// `proc_loginuid_read` and `proc_sessionid_read`: the audit login uid as the reader's user
/// namespace sees it (`AUDIT_UID_UNSET` when unset), and the session id; no newline.
pub(super) fn loginuid(process: &crate::Process, reader: &Arc<UserNs>) -> Vec<u8> {
    let uid = process.ident.lock_or_recover().loginuid;
    let shown = uid.map_or(u32::MAX, |uid| reader.munged_uid(uid));
    shown.to_string().into_bytes()
}

pub(super) fn sessionid(process: &crate::Process) -> Vec<u8> {
    process
        .ident
        .lock_or_recover()
        .sessionid
        .to_string()
        .into_bytes()
}

/// fs/proc/base.c `proc_pid_syscall`: the blocked call's registers, or the task's state.
/// Wasm has no instruction pointer; an untraced call has no sampled shadow stack pointer.
pub(super) fn syscall(task: &TaskInfo) -> Vec<u8> {
    let current = *task.syscall.lock_or_recover();
    if let Some(call) = current {
        let call = task
            .signals
            .group()
            .with_trace(task.id, |t| t.call)
            .flatten()
            .unwrap_or(call);
        let nr = call.nr as i32;
        if nr < 0 {
            return format!("{nr} 0x{:x} 0x0\n", call.sp).into_bytes();
        }
        let [a, b, c, d, e, f] = call.args;
        return format!(
            "{nr} 0x{a:x} 0x{b:x} 0x{c:x} 0x{d:x} 0x{e:x} 0x{f:x} 0x{:x} 0x0\n",
            call.sp
        )
        .into_bytes();
    }
    if task.signals.is_stopped() {
        b"-1 0x0 0x0\n".to_vec()
    } else {
        b"running\n".to_vec()
    }
}

/// fs/proc/base.c `do_io_accounting`: process totals include retired and live threads;
/// /proc/<pid>/task/<tid>/io shows just that thread.
pub(super) fn io(
    source: &Source,
    scope: super::node::Scope,
    process: &crate::Process,
) -> wasixv2_vfs::FsResult<Vec<u8>> {
    let state = source.state.upgrade().ok_or(wasixv2_vfs::Errno::ESRCH)?;
    let state = state.read_or_recover();
    let counts = match scope.tid {
        Some(tid) => state
            .tasks
            .get(&wasixv2_syscalls::TaskId(tid))
            .filter(|t| t.process.pid.0 == scope.pid)
            .ok_or(wasixv2_vfs::Errno::ESRCH)?
            .ioac
            .counts(),
        None => {
            let mut counts = process.ioac.counts();
            for task in state
                .tasks
                .values()
                .filter(|t| t.process.pid.0 == scope.pid)
            {
                for (total, count) in counts.iter_mut().zip(task.ioac.counts()) {
                    *total = total.wrapping_add(count);
                }
            }
            counts
        }
    };
    let [rchar, wchar, syscr, syscw] = counts;
    Ok(format!(
        "rchar: {rchar}\nwchar: {wchar}\nsyscr: {syscr}\nsyscw: {syscw}\nread_bytes: 0\n\
         write_bytes: 0\ncancelled_write_bytes: 0\n"
    )
    .into_bytes())
}

/// `show_mountstats` without filesystem statistics, which only network filesystems have.
pub(super) fn mountstats(source: &Source, ns: &MntNs, root: &Path) -> Vec<u8> {
    let Some(vfs) = source.vfs.upgrade() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in vfs.mount_info(ns, root) {
        let mount = &entry.mount;
        let point = vfs.d_path(&mount.root_path(), root);
        if point.starts_with(b"(unreachable)") {
            continue;
        }
        let devname = match mount.devname() {
            b"" => b"no device".as_slice(),
            name => name,
        };
        out.extend_from_slice(b"device ");
        out.extend_from_slice(&escape(devname));
        out.extend_from_slice(b" mounted on ");
        out.extend_from_slice(&escape(&point));
        out.extend_from_slice(format!(" with fstype {}\n", mount.sb().info.name).as_bytes());
    }
    out
}

/// `show_smap` for each area, and `show_smaps_rollup` for all of them at once: every page of
/// an area is resident, private ones dirty, as nothing is ever swapped out or dropped.
pub(super) fn smaps(mm: Option<&Mm>, rollup: bool) -> Vec<u8> {
    let regions = mm.map(Mm::regions).unwrap_or_default();
    let mut out = String::new();
    if rollup {
        // An empty address space shows a header of zeros (`empty_set`).
        let start = regions.first().map_or(0, |r| r.start);
        let end = regions.last().map_or(0, |r| r.end);
        let size: u64 = regions.iter().map(|r| r.end - r.start).sum();
        let shared: u64 = regions
            .iter()
            .filter(|r| r.shared)
            .map(|r| r.end - r.start)
            .sum();
        let locked: u64 = regions
            .iter()
            .filter(|r| r.locked)
            .map(|r| r.end - r.start)
            .sum();
        let line = format!("{start:08x}-{end:08x} ---p 00000000 00:00 0");
        let _ = writeln!(out, "{line:<73}[rollup]");
        usage(&mut out, size, shared, locked, true);
        return out.into_bytes();
    }
    for region in regions {
        let name = match region.kind {
            RegionKind::Image => "[image]",
            RegionKind::Heap => "[heap]",
            RegionKind::Anon => "",
        };
        let flag = |on: bool, c: char| if on { c } else { '-' };
        let perms = format!(
            "{}{}-{}",
            flag(region.read, 'r'),
            flag(region.write, 'w'),
            if region.shared { 's' } else { 'p' }
        );
        let line = format!(
            "{:08x}-{:08x} {perms} 00000000 00:00 0",
            region.start, region.end
        );
        let _ = match name {
            "" => writeln!(out, "{line}"),
            name => writeln!(out, "{line:<73}{name}"),
        };
        let size = region.end - region.start;
        let shared = if region.shared { size } else { 0 };
        let locked = if region.locked { size } else { 0 };
        usage(&mut out, size, shared, locked, false);
        let mut flags = String::new();
        for (on, name) in [
            (region.read, "rd"),
            (region.write, "wr"),
            (region.shared, "sh"),
            (true, "mr"),
            (true, "mw"),
            (true, "me"),
            (region.shared, "ms"),
            (region.locked, "lo"),
            (region.wipe_on_fork, "wf"),
            (!region.shared, "ac"),
        ] {
            if on {
                let _ = write!(flags, "{name} ");
            }
        }
        let _ = writeln!(out, "VmFlags: {flags}");
    }
    out.into_bytes()
}

/// The page counts of `__show_smap` for `size` bytes, `shared` of them shared.
fn usage(out: &mut String, size: u64, shared: u64, locked: u64, rollup: bool) {
    let kb = |bytes: u64| bytes / 1024;
    let private = size - shared;
    let mut rows = vec![
        ("Rss", kb(size)),
        ("Pss", kb(size)),
        ("Pss_Dirty", kb(private)),
    ];
    if rollup {
        rows.extend([
            ("Pss_Anon", kb(private)),
            ("Pss_File", 0),
            ("Pss_Shmem", kb(shared)),
        ]);
    } else {
        rows.insert(0, ("Size", kb(size)));
        rows.insert(1, ("KernelPageSize", kb(PAGE_SIZE)));
        rows.insert(2, ("MMUPageSize", kb(PAGE_SIZE)));
    }
    rows.extend([
        ("Shared_Clean", 0),
        ("Shared_Dirty", kb(shared)),
        ("Private_Clean", 0),
        ("Private_Dirty", kb(private)),
        ("Referenced", kb(size)),
        ("Anonymous", kb(private)),
        ("KSM", 0),
        ("LazyFree", 0),
        ("AnonHugePages", 0),
        ("ShmemPmdMapped", 0),
        ("FilePmdMapped", 0),
        ("Shared_Hugetlb", 0),
        ("Private_Hugetlb", 0),
        ("Swap", 0),
        ("SwapPss", 0),
        ("Locked", kb(locked)),
    ]);
    for (name, value) in rows {
        let _ = writeln!(out, "{:<16}{value:8} kB", format!("{name}:"));
    }
    if !rollup {
        let _ = writeln!(out, "THPeligible:    0");
    }
}

/// `show_timer` for each POSIX timer of the process, by id.
pub(super) fn timers(task: &TaskInfo, group: i32, nr: impl Fn(u32) -> i32) -> Vec<u8> {
    let mut out = String::new();
    let timers = task.signals.group().with_posix_timers(|table| table.all());
    for timer in timers {
        let (id, clock, notify, signo, value, target) = timer.proc_show();
        let kind = match notify {
            Some(true) => "tid",
            _ => "pid",
        };
        let how = match notify {
            None => "none",
            Some(_) => "signal",
        };
        let pid = target.map_or(group, |t| nr(t.0));
        let _ = write!(
            out,
            "ID: {id}\nsignal: {signo}/{value:08x}\nnotify: {how}/{kind}.{pid}\nClockID: {clock}\n"
        );
    }
    out.into_bytes()
}
