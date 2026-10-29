//! The integer sysctls, `/proc/sys/kernel/random`, `fs.file-nr`, `vm.drop_caches`,
//! `/proc/sysrq-trigger`, the static `/proc` files and `/sys/kernel`.

use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, get, put},
    proc::ForkableMemory,
    *,
};

const CWD: Fd = Fd(-100);
const SIGCHLD: u32 = Signal::SIGCHLD.0 as u32;

struct P {
    task: TaskId,
    memory: ForkableMemory,
}

impl P {
    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    fn open(&self, kernel: &Kernel, at: &str, flags: OpenFlags) -> SysResult<Fd> {
        put(&self.memory.0, 0x1000, format!("{at}\0").as_bytes());
        block_on(kernel.openat(&self.caller(), CWD, GuestPtr::new(0x1000), flags, 0))
    }

    fn read(&self, kernel: &Kernel, at: &str) -> SysResult<String> {
        let fd = self.open(kernel, at, OpenFlags::O_RDONLY)?;
        let n = block_on(kernel.read(&self.caller(), fd, UserSliceMut::new(0x4000, 0x4000)))?;
        block_on(kernel.close(&self.caller(), fd)).unwrap();
        Ok(String::from_utf8(get(&self.memory.0, 0x4000, n as usize)).unwrap())
    }

    fn write(&self, kernel: &Kernel, at: &str, text: &str) -> SysResult<u32> {
        let fd = self.open(kernel, at, OpenFlags::O_WRONLY)?;
        put(&self.memory.0, 0x2000, text.as_bytes());
        let written = block_on(kernel.write(
            &self.caller(),
            fd,
            UserSlice::new(0x2000, text.len() as u32),
        ));
        block_on(kernel.close(&self.caller(), fd)).unwrap();
        written
    }

    fn fork(&self, kernel: &Kernel) -> SysResult<TaskId> {
        let null = GuestPtrMut::NULL;
        block_on(kernel.clone(&self.caller(), SIGCHLD, 0, null, 0, null))?;
        let mut child = None;
        while let Some(action) = kernel.take_pending(self.task) {
            if let GuestAction::Fork { child: task, .. } = action {
                child = Some(task);
            }
        }
        Ok(child.expect("fork left the child"))
    }

    fn log(&self, kernel: &Kernel) -> String {
        let n = block_on(kernel.syslog(&self.caller(), 3, GuestPtrMut::new(0x6000), 8192)).unwrap();
        String::from_utf8(get(&self.memory.0, 0x6000, n as usize)).unwrap()
    }
}

fn machine() -> (Kernel, P) {
    let (kernel, task, _) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(0, 0)));
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    block_on(vfs.mount(b"/sys", kernel.sysfs(), MountFlags::default(), true)).unwrap();
    let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
    (kernel, P { task, memory })
}

/// `kernel.threads-max` bounds the tasks `fork` makes (`EAGAIN`), within `[20, 2^30)`.
#[test]
fn threads_max_bounds_fork() {
    let (kernel, p) = machine();
    assert_eq!(
        p.read(&kernel, "/proc/sys/kernel/threads-max"),
        Ok("65536\n".into())
    );
    assert_eq!(
        errno(p.write(&kernel, "/proc/sys/kernel/threads-max", "19")),
        Errno::EINVAL
    );
    assert_eq!(
        p.write(&kernel, "/proc/sys/kernel/threads-max", "20"),
        Ok(2)
    );
    for _ in 1..20 {
        p.fork(&kernel).unwrap();
    }
    assert_eq!(errno(p.fork(&kernel)), Errno::EAGAIN);
}

/// The tunables keep what is written within Linux's range; `write_wakeup_threshold` takes
/// writes and ignores them; `sched_rr_timeslice_ms` of 0 is the default again.
#[test]
fn tunables_take_values_in_range() {
    let (kernel, p) = machine();
    assert_eq!(
        p.read(&kernel, "/proc/sys/vm/swappiness"),
        Ok("60\n".into())
    );
    assert_eq!(p.write(&kernel, "/proc/sys/vm/swappiness", "10\n"), Ok(3));
    assert_eq!(
        p.read(&kernel, "/proc/sys/vm/swappiness"),
        Ok("10\n".into())
    );
    assert_eq!(
        errno(p.write(&kernel, "/proc/sys/vm/swappiness", "201")),
        Errno::EINVAL
    );
    assert_eq!(
        errno(p.write(&kernel, "/proc/sys/vm/swappiness", "x")),
        Errno::EINVAL
    );
    let threshold = "/proc/sys/kernel/random/write_wakeup_threshold";
    assert_eq!(p.write(&kernel, threshold, "64"), Ok(2));
    assert_eq!(p.read(&kernel, threshold), Ok("256\n".into()));
    let rr = "/proc/sys/kernel/sched_rr_timeslice_ms";
    assert_eq!(p.write(&kernel, rr, "25"), Ok(2));
    assert_eq!(p.read(&kernel, rr), Ok("25\n".into()));
    assert_eq!(p.write(&kernel, rr, "0"), Ok(1));
    assert_eq!(p.read(&kernel, rr), Ok("100\n".into()));
    assert_eq!(
        p.read(&kernel, "/proc/sys/fs/inotify/max_queued_events"),
        Ok("16384\n".into())
    );
}

/// `boot_id` stays for the machine's life, `uuid` is new on each read; both are version 4.
#[test]
fn random_ids_are_uuids() {
    let (kernel, p) = machine();
    let boot = p.read(&kernel, "/proc/sys/kernel/random/boot_id").unwrap();
    assert_eq!(
        p.read(&kernel, "/proc/sys/kernel/random/boot_id"),
        Ok(boot.clone())
    );
    assert_eq!(boot.len(), 37);
    assert_eq!(&boot[14..15], "4");
    let uuid = p.read(&kernel, "/proc/sys/kernel/random/uuid").unwrap();
    assert_eq!(&uuid[14..15], "4");
    assert_eq!(
        p.read(&kernel, "/proc/sys/kernel/random/entropy_avail"),
        Ok("256\n".into())
    );
    let files = p.read(&kernel, "/proc/sys/fs/file-nr").unwrap();
    let open: usize = files.split('\t').next().unwrap().parse().unwrap();
    assert!(open >= 3, "{files}");
}

/// The files of what the machine does not have: no swap, block devices, interrupts or
/// printk console; the character devices it has.
#[test]
fn static_proc_files_show_the_machine() {
    let (kernel, p) = machine();
    assert_eq!(
        p.read(&kernel, "/proc/swaps"),
        Ok("Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n".into())
    );
    assert_eq!(
        p.read(&kernel, "/proc/partitions"),
        Ok("major minor  #blocks  name\n\n".into())
    );
    assert_eq!(p.read(&kernel, "/proc/consoles"), Ok(String::new()));
    let devices = p.read(&kernel, "/proc/devices").unwrap();
    assert!(
        devices.starts_with("Character devices:\n  1 mem\n"),
        "{devices}"
    );
    assert!(devices.ends_with("\nBlock devices:\n"), "{devices}");
    let softirqs = p.read(&kernel, "/proc/softirqs").unwrap();
    assert!(softirqs.contains("       TIMER:"), "{softirqs}");
    assert!(
        p.read(&kernel, "/proc/vmstat")
            .unwrap()
            .starts_with("nr_free_pages ")
    );
    assert!(
        p.read(&kernel, "/proc/interrupts")
            .unwrap()
            .contains("CPU0")
    );
}

/// `/proc/sysrq-trigger` runs the actions it has, logging them; `vm.drop_caches` logs who
/// asked.
#[test]
fn sysrq_and_drop_caches_act_and_log() {
    let (kernel, p) = machine();
    let child = p.fork(&kernel).unwrap();
    assert_eq!(p.write(&kernel, "/proc/sysrq-trigger", "h"), Ok(1));
    assert!(p.log(&kernel).contains("sysrq: HELP : loglevel(0-9) "));
    assert_eq!(p.write(&kernel, "/proc/sysrq-trigger", "5"), Ok(1));
    assert_eq!(kernel.printk.levels()[0], 5);
    // Everyone but init, which the first process is here.
    assert_eq!(p.write(&kernel, "/proc/sysrq-trigger", "e"), Ok(1));
    assert!(p.log(&kernel).contains("sysrq: Terminate All Tasks\n"));
    assert!(matches!(
        kernel.take_pending(child),
        Some(GuestAction::Exit(_))
    ));
    assert!(!matches!(
        kernel.take_pending(p.task),
        Some(GuestAction::Exit(_))
    ));

    assert_eq!(p.write(&kernel, "/proc/sys/vm/drop_caches", "3"), Ok(1));
    assert!(p.log(&kernel).contains("] test (1): drop_caches: 3\n"));
    assert_eq!(
        errno(p.write(&kernel, "/proc/sys/vm/drop_caches", "5")),
        Errno::EINVAL
    );
}

/// `/sys/kernel/mm` is there and empty; `/sys/kernel/cgroup` tells what cgroup2 delegates and
/// takes; `/sys/fs/cgroup` is where it is mounted.
#[test]
fn sys_kernel_has_mm_and_cgroup() {
    let (kernel, p) = machine();
    assert!(
        p.open(&kernel, "/sys/kernel/mm", OpenFlags::O_DIRECTORY)
            .is_ok()
    );
    assert!(
        p.open(&kernel, "/sys/fs/cgroup", OpenFlags::O_DIRECTORY)
            .is_ok()
    );
    assert_eq!(
        p.read(&kernel, "/sys/kernel/cgroup/delegate"),
        Ok("cgroup.procs\ncgroup.threads\ncgroup.subtree_control\n".into())
    );
    let features = p.read(&kernel, "/sys/kernel/cgroup/features").unwrap();
    assert!(features.starts_with("nsdelegate\n") && features.ends_with("pids_localevents\n"));
}
