//! The host's console: a terminal whose device is a pair of host byte streams, such as the
//! terminal `wasmer run` was started on, in raw mode. Linux's `hvc0`, the console a hypervisor
//! gives a virtual machine: the host only moves bytes, and everything a terminal does with
//! them is the kernel's.
//!
//! Like getty, [`Kernel::attach_console`] opens it for a new process, which leads a session
//! and acquires it as its controlling terminal.

use std::{
    io,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

use wasixv2_abi::{OpenFlags, Signal, Winsize};
use wasixv2_host::{ByteSink, ByteSource, WindowSize};
use wasixv2_syscalls::{Caller, Errno, Fd, SysErr, TaskId};
use wasixv2_vfs::{Device, LookupFlags, OpenHow};
use wasixv2_wait::{NoInterrupt, PollMask, WaitQueue, Waiter};

use super::{Driver, Tty};
use crate::{Kernel, file::OpenFile, stdio::QueueWaker, sync::MutexExt};

/// `HVC_MAJOR`: the hypervisor consoles, `/dev/hvc0` first.
pub(crate) const HVC_MAJOR: u32 = 229;
const NAME: &str = "hvc0";

/// How much of the host's input the pump takes at once.
const INPUT_CHUNK: usize = 4096;

/// How long a run's end waits for the console's output to reach the host: `tty_port`'s
/// default `closing_wait` (`drivers/tty/tty_port.c`: `tty_port_init`), 30 seconds.
const CLOSING_WAIT: Duration = Duration::from_secs(30);

/// The console's device: the host streams.
pub(crate) struct Console {
    input: Arc<dyn ByteSource>,
    output: Arc<dyn ByteSink>,
    /// The input pump waits here: for the source to have input, and for the discipline to
    /// have room for it.
    pump_wait: WaitQueue,
    waker: Waker,
    pumping: AtomicBool,
    /// Where `create_device_nodes` made the console's node, which `attach_console` opens.
    node: Mutex<Option<Vec<u8>>>,
}

impl Console {
    fn new(console: wasixv2_host::Console) -> Self {
        let pump_wait = WaitQueue::new();
        Self {
            input: console.input,
            output: console.output,
            waker: Waker::from(Arc::new(QueueWaker(pump_wait.clone()))),
            pump_wait,
            pumping: AtomicBool::new(false),
            node: Mutex::default(),
        }
    }

    /// Writes all of `data` to the host.
    pub(super) async fn write(&self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            match self.output.write(data).await? {
                0 => return Err(io::ErrorKind::WriteZero.into()),
                n => data = &data[n..],
            }
        }
        Ok(())
    }

    pub(super) fn unthrottle(&self) {
        self.pump_wait.wake(PollMask::READABLE);
    }
}

fn winsize(size: WindowSize) -> Winsize {
    Winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.xpixel,
        ws_ypixel: size.ypixel,
    }
}

impl Tty {
    fn console(&self) -> &Console {
        match &self.driver {
            Driver::Console(console) => console,
            _ => unreachable!("the console's terminal"),
        }
    }
}

/// Starts the console's input pump, once: the host's input goes through the line discipline
/// as it arrives, whether or not anyone reads, so `^C` interrupts a busy program. It runs on a
/// host thread of its own where the host has them.
fn start_pump(tty: &Arc<Tty>) {
    if tty.console().pumping.swap(true, Ordering::Relaxed) {
        return;
    }
    let weak = Arc::downgrade(tty);
    let start: wasixv2_host::ThreadStart = Box::new(move || Box::pin(pump(weak)));
    let executor = tty.host.executor();
    if executor.spawn_thread(start).is_err() {
        executor.spawn_local(Box::pin(pump(Arc::downgrade(tty))));
    }
}

/// The pump: reads the host's input and hands it to the discipline, holding what does not fit
/// until a reader makes room. The input's end means the terminal went away: a hangup.
async fn pump(tty: Weak<Tty>) {
    let waiter = Waiter::new();
    let _registration = match tty.upgrade() {
        Some(tty) => waiter.register(&tty.console().pump_wait, PollMask::empty()),
        None => return,
    };
    let mut held = Vec::new();
    let mut chunk = vec![0; INPUT_CHUNK];
    loop {
        let Some(live) = tty.upgrade() else { return };
        if held.is_empty() {
            let console = live.console();
            let mut cx = Context::from_waker(&console.waker);
            match console.input.poll_read(&mut cx, &mut chunk) {
                Poll::Ready(Ok(0) | Err(_)) => {
                    live.hangup(false);
                    return;
                }
                Poll::Ready(Ok(n)) => held.extend_from_slice(&chunk[..n]),
                Poll::Pending => {
                    drop(live);
                    waiter.sleep(&NoInterrupt, None).await;
                    continue;
                }
            }
        }
        let taken = live.receive(&held).await;
        held.drain(..taken);
        if taken == 0 {
            drop(live);
            waiter.sleep(&NoInterrupt, None).await;
        }
    }
}

impl Kernel {
    /// Gives the kernel the host's console, `console`, a terminal (`/dev/hvc0`, device 229:0)
    /// with Linux's default settings and `console.size` as its window size. Processes reach it
    /// through [`Kernel::attach_console`]; [`Kernel::create_device_nodes`] makes its node.
    pub fn with_console(mut self, console: wasixv2_host::Console) -> Self {
        let size = winsize(console.size);
        let driver = Driver::Console(Console::new(console));
        let device = Device {
            major: HVC_MAJOR,
            minor: 0,
        };
        let tty = Tty::new(
            device,
            super::ldisc::std_termios(),
            size,
            driver,
            self.jobs.clone(),
            self.host.clone(),
        );
        self.console = Some(Arc::new(tty));
        self
    }

    pub(crate) fn console(&self) -> Option<&Arc<Tty>> {
        self.console.as_ref()
    }

    /// Where the console's node goes in the directory `dir`, made by `create_device_nodes`.
    pub(crate) fn console_node(&self, dir: &[u8]) -> Option<(&'static str, Device)> {
        let console = self.console.as_ref()?;
        let mut path = dir.to_vec();
        if !path.ends_with(b"/") {
            path.push(b'/');
        }
        path.extend_from_slice(NAME.as_bytes());
        *console.console().node.lock_or_recover() = Some(path);
        Some((NAME, console.device()))
    }

    /// Opens the console for the process `task` as getty opens a terminal for a login: the
    /// descriptors `fds` names of 0, 1 and 2 become one open file of it, read-write, replacing
    /// what they were, and a session leader without a controlling terminal acquires it, its
    /// group in the foreground. Starts moving the host's input to the console. Fails with
    /// `ENODEV` without a console, and with `ENOENT` when its node was never made.
    pub async fn attach_console(&self, task: TaskId, fds: [bool; 3]) -> Result<(), Errno> {
        let tty = self.console.as_ref().ok_or(Errno::ENODEV)?;
        let node = tty.console().node.lock_or_recover().clone();
        let node = node.ok_or(Errno::ENOENT)?;
        let memory = crate::process::NoMemory;
        let caller = Caller::new(task, &memory);
        let env = self.task_env(task);
        let how = OpenHow {
            flags: OpenFlags::O_RDWR,
            mode: 0,
            lookup: LookupFlags::follow(),
        };
        let file = self.vfs.open(&env.cx(), &env.fs.root, &node, &how).await?;
        let backing = self
            .attach(&caller, file, OpenFlags::O_RDWR)
            .await
            .map_err(|err| match err {
                SysErr::Errno(errno) => errno,
                _ => Errno::EINTR,
            })?;
        let file = Arc::new(OpenFile::new(backing, OpenFlags::O_RDWR));
        let pid = self.process(task).pid;
        let replaced: Vec<_> = {
            let files = self.files(task);
            let mut files = files.lock_or_recover();
            (0..3)
                .filter(|&fd| fds[fd])
                .filter_map(|fd| files.install_at(Fd(fd as i32), file.clone(), false))
                .collect()
        };
        for old in replaced {
            self.closed_by(pid, &old);
        }
        start_pump(tty);
        Ok(())
    }

    /// The console's window changed to `size`: its foreground process group gets SIGWINCH.
    pub fn console_resize(&self, size: WindowSize) {
        if let Some(tty) = &self.console {
            tty.resize(winsize(size));
        }
    }

    /// The console went away, as a terminal whose line drops: it is hung up. Its session's
    /// leader gets SIGHUP and SIGCONT, and the files open on it end.
    pub fn console_hangup(&self) {
        if let Some(tty) = &self.console {
            tty.hangup(false);
        }
    }

    /// Sends `sig` to the console's foreground process group, as its line discipline sends
    /// the signal of `^C` (`kill_pgrp` with `SI_KERNEL`). Returns whether some process has it
    /// pending now. `ESRCH` without a console or a foreground group.
    pub fn console_signal(&self, sig: Signal) -> Result<bool, Errno> {
        let sig = crate::signal::check_signal(sig)?;
        let tty = self.console.as_ref().ok_or(Errno::ESRCH)?;
        let pgrp = tty.pgrp().ok_or(Errno::ESRCH)?;
        if tty.jobs.pgrp_groups(pgrp as i32).is_empty() {
            return Err(Errno::ESRCH);
        }
        Ok(tty.jobs.kill_pgrp(pgrp, sig))
    }

    /// Whether `sig` is pending, not yet taken, for a process of the console's foreground
    /// process group.
    pub fn console_signal_pending(&self, sig: Signal) -> bool {
        let Some(pgrp) = self.console.as_ref().and_then(|tty| tty.pgrp()) else {
            return false;
        };
        self.jobs
            .pgrp_groups(pgrp as i32)
            .iter()
            .any(|(_, group)| group.is_pending(sig.0))
    }

    /// Waits until what the console holds for the host has reached it: the write under way
    /// and the echoes of input the console received, such as the `^C` whose signal ended the
    /// program, which the input pump writes after it signals, as Linux's line discipline does.
    /// An embedder calls this when a run ends, before it exits, as Linux's `hvc_close` waits
    /// (`drivers/tty/hvc/hvc_console.c`, `tty_wait_until_sent`); the wait is bounded by
    /// `tty_port`'s default closing wait, 30 seconds, for a host that takes nothing. Returns at
    /// once without a console.
    pub async fn console_wait_until_sent(&self) {
        if let Some(tty) = &self.console {
            tty.wait_until_sent(CLOSING_WAIT).await;
        }
    }

    /// Whether the kernel has a console.
    pub fn has_console(&self) -> bool {
        self.console.is_some()
    }
}
