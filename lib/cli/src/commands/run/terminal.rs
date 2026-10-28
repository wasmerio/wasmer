//! The terminal `wasmer run` was started on, handed to the WASIX v2 machine as its console.
//!
//! The guest's line discipline does what a terminal does with typed input (echo, line editing,
//! `^C`), so the host terminal goes into raw mode for as long as the guest runs, and passes
//! bytes only. It is restored on every way out: when the run ends or fails, from the panic
//! hook, and around a stop of `wasmer` itself (`SIGTSTP`, then `SIGCONT`).

use std::{
    io,
    os::fd::RawFd,
    sync::{Arc, Mutex, Once, TryLockError},
};

use wasixv2::{ByteSink, Console, LocalFuture, Stdio, WindowSize};

/// The terminal's settings to restore and the raw ones the guest runs with, while a
/// [`RawMode`] is in force; the panic hook and signal handlers reach them here.
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct Active {
    fd: RawFd,
    saved: libc::termios,
    raw: libc::termios,
}

/// The terminal on standard input, when `wasmer run` may take it for the guest.
#[derive(Clone, Copy)]
pub(super) struct HostTerminal {
    fd: RawFd,
    /// Which of descriptors 0, 1 and 2 are this terminal.
    fds: [bool; 3],
}

impl HostTerminal {
    /// The terminal on descriptor 0, unless it is no terminal or `wasmer` runs in the
    /// background of it: taking it there would stop `wasmer` with `SIGTTOU`, as a background
    /// job that changes its terminal stops.
    pub(super) fn detect() -> Option<Self> {
        const STDIN: RawFd = 0;
        // SAFETY: plain calls on a descriptor number.
        let foreground =
            unsafe { libc::isatty(STDIN) == 1 && libc::tcgetpgrp(STDIN) == libc::getpgrp() };
        if !foreground {
            return None;
        }
        let device = rdev(STDIN)?;
        let on_it = |fd| unsafe { libc::isatty(fd) == 1 } && rdev(fd) == Some(device);
        Some(Self {
            fd: STDIN,
            fds: [true, on_it(1), on_it(2)],
        })
    }

    /// The console for the machine, and which of a program's descriptors 0, 1 and 2 are on
    /// it: what is typed comes from `stdio`'s standard input, and what the guest shows goes to
    /// the first of standard output, standard error and the terminal itself that shows it.
    pub(super) fn console(&self, stdio: &Stdio) -> io::Result<(Console, [bool; 3])> {
        let output: Arc<dyn ByteSink> = match self.fds {
            [_, true, _] => stdio.stdout.clone(),
            [_, _, true] => stdio.stderr.clone(),
            _ => Arc::new(TtyWriter::open()?),
        };
        let console = Console {
            input: stdio.stdin.clone(),
            output,
            size: window_size(self.fd),
        };
        Ok((console, self.fds))
    }

    /// Puts the terminal in raw mode until the returned guard goes.
    pub(super) fn raw(&self) -> io::Result<RawMode> {
        RawMode::enter(self.fd)
    }

    pub(super) fn size(&self) -> WindowSize {
        window_size(self.fd)
    }
}

/// The device number of the terminal open on `fd`.
fn rdev(fd: RawFd) -> Option<libc::dev_t> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `fstat` fills `stat` when it succeeds.
    match unsafe { libc::fstat(fd, stat.as_mut_ptr()) } {
        0 => Some(unsafe { stat.assume_init() }.st_rdev),
        _ => None,
    }
}

/// The window size of the terminal on `fd`, zero where unknown.
fn window_size(fd: RawFd) -> WindowSize {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `TIOCGWINSZ` fills a `winsize`.
    unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    WindowSize {
        rows: ws.ws_row,
        cols: ws.ws_col,
        xpixel: ws.ws_xpixel,
        ypixel: ws.ws_ypixel,
    }
}

/// The terminal in raw mode; dropping it restores the settings it found.
pub(super) struct RawMode(());

impl RawMode {
    /// `cfmakeraw` on the terminal at `fd`, after saving its settings, which the panic hook
    /// restores from then on.
    pub(super) fn enter(fd: RawFd) -> io::Result<Self> {
        let saved = get(fd)?;
        let mut raw = saved;
        // SAFETY: `cfmakeraw` changes the flags of a valid `termios`.
        unsafe { libc::cfmakeraw(&mut raw) };
        install_panic_hook();
        *ACTIVE.lock().unwrap() = Some(Active { fd, saved, raw });
        if let Err(err) = set(fd, &raw, libc::TCSANOW) {
            ACTIVE.lock().unwrap().take();
            return Err(err);
        }
        Ok(Self(()))
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(active) = ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).take() {
            restore_settings(&active);
        }
    }
}

/// Gives the terminal its own settings back while raw mode is in force, before `wasmer` stops.
pub(super) fn suspend() {
    if let Some(active) = *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) {
        restore_settings(&active);
    }
}

/// Raw mode again after [`suspend`], or after whoever had the terminal while `wasmer` was
/// stopped changed it, as a shell does.
pub(super) fn resume() {
    if let Some(active) = *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) {
        let _ = set(active.fd, &active.raw, libc::TCSANOW);
    }
}

/// `TCSADRAIN`: what the guest wrote is out before the settings change back.
fn restore_settings(active: &Active) {
    let _ = set(active.fd, &active.saved, libc::TCSADRAIN);
}

fn get(fd: RawFd) -> io::Result<libc::termios> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `tcgetattr` fills `termios` when it succeeds.
    match unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } {
        0 => Ok(unsafe { termios.assume_init() }),
        _ => Err(io::Error::last_os_error()),
    }
}

fn set(fd: RawFd, termios: &libc::termios, when: libc::c_int) -> io::Result<()> {
    // SAFETY: `tcsetattr` reads a valid `termios`.
    match unsafe { libc::tcsetattr(fd, when, termios) } {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Restores the terminal before the previous hook reports the panic, so the report reads as
/// it should and the shell gets its terminal back.
fn install_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // A panic while the lock is held must not deadlock here.
            let active = match ACTIVE.try_lock() {
                Ok(mut active) => active.take(),
                Err(TryLockError::Poisoned(active)) => active.into_inner().take(),
                Err(TryLockError::WouldBlock) => None,
            };
            if let Some(active) = active {
                restore_settings(&active);
            }
            previous(info);
        }));
    });
}

/// Writes to the controlling terminal, for a console whose standard output and error go
/// elsewhere.
struct TtyWriter(tokio::sync::Mutex<tokio::fs::File>);

impl TtyWriter {
    fn open() -> io::Result<Self> {
        let tty = std::fs::OpenOptions::new().write(true).open("/dev/tty")?;
        Ok(Self(tokio::sync::Mutex::new(tokio::fs::File::from_std(
            tty,
        ))))
    }
}

impl ByteSink for TtyWriter {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        use tokio::io::AsyncWriteExt as _;
        Box::pin(async move {
            let mut tty = self.0.lock().await;
            let n = tty.write(data).await?;
            tty.flush().await?;
            Ok(n)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pseudo-terminal pair: its slave stands for the user's terminal.
    struct Pty {
        master: RawFd,
        slave: RawFd,
    }

    impl Pty {
        fn open() -> Self {
            let (mut master, mut slave) = (0, 0);
            // SAFETY: `openpty` fills both descriptors when it succeeds.
            let ret = unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            assert_eq!(ret, 0, "{}", io::Error::last_os_error());
            Self { master, slave }
        }
    }

    impl Drop for Pty {
        fn drop(&mut self) {
            // SAFETY: the pair's own descriptors.
            unsafe {
                libc::close(self.master);
                libc::close(self.slave);
            }
        }
    }

    fn same(a: &libc::termios, b: &libc::termios) -> bool {
        (a.c_iflag, a.c_oflag, a.c_cflag, a.c_lflag, a.c_cc)
            == (b.c_iflag, b.c_oflag, b.c_cflag, b.c_lflag, b.c_cc)
    }

    fn is_raw(termios: &libc::termios) -> bool {
        termios.c_lflag & (libc::ICANON | libc::ECHO | libc::ISIG) == 0
            && termios.c_oflag & libc::OPOST == 0
    }

    /// One test, as raw mode is process-wide state: the guard, a stop, and a panic each give
    /// the terminal its settings back.
    #[test]
    fn the_terminal_is_restored_on_every_way_out() {
        let pty = Pty::open();
        let cooked = get(pty.slave).unwrap();
        assert!(!is_raw(&cooked));

        let raw = RawMode::enter(pty.slave).unwrap();
        assert!(is_raw(&get(pty.slave).unwrap()));
        suspend();
        assert!(same(&get(pty.slave).unwrap(), &cooked), "stopped: restored");
        resume();
        assert!(is_raw(&get(pty.slave).unwrap()), "continued: raw again");
        drop(raw);
        assert!(same(&get(pty.slave).unwrap(), &cooked), "dropped: restored");

        let raw = RawMode::enter(pty.slave).unwrap();
        let panicked = std::panic::catch_unwind(|| panic!("the panic path"));
        assert!(panicked.is_err());
        assert!(
            same(&get(pty.slave).unwrap(), &cooked),
            "the panic hook restored"
        );
        // The hook took the settings: the guard has nothing left to restore.
        drop(raw);
        assert!(same(&get(pty.slave).unwrap(), &cooked));
    }
}
