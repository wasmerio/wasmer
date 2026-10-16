//! `tty_get`, `tty_set`: v1's one terminal state for the process, which wasix-libc reads for
//! `isatty` (per stdio descriptor) and `tcgetattr`, and writes for `tcsetattr`.
//!
//! The state comes from Linux's terminal ioctls on descriptors 0, 1 and 2: `TIOCGWINSZ` says
//! whether each is a terminal and how big the first one is, and `TCGETS`/`TCSETS` on the first
//! one carry the modes v1 knows (`ECHO`, `ICANON`, and `IGNCR` as "line feeds"). Without a
//! terminal, the modes are compat state that `tty_set` keeps, starting at v1's defaults, and
//! the size is v1's default 80x25.

use std::sync::Mutex;

use wasixv2_abi::{Fd, IoctlRequest, Termios, TermiosIflag, TermiosLflag, Winsize};
use wasixv2_syscalls::{Control, SysErr, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{Bool, proc::Tty},
    errno::V1Result,
    shim::{self, Env, Frame},
};

/// The terminal modes v1 knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    pub echo: bool,
    pub line_buffered: bool,
    pub line_feeds: bool,
}

impl Default for Modes {
    /// v1's defaults.
    fn default() -> Self {
        Self {
            echo: false,
            line_buffered: false,
            line_feeds: true,
        }
    }
}

/// The modes `tty_set` stored for a process without a terminal.
#[derive(Debug, Default)]
pub struct State {
    modes: Mutex<Modes>,
}

impl State {
    pub(crate) fn fork(&self) -> Self {
        Self {
            modes: Mutex::new(*self.modes.lock().unwrap()),
        }
    }
}

const DEFAULT_COLS: u32 = 80;
const DEFAULT_ROWS: u32 = 25;
const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 600;

fn bool(value: bool) -> Bool {
    if value { Bool::TRUE } else { Bool::FALSE }
}

/// Which of the stdio descriptors are terminals, and the window size of the first that is.
async fn stdio<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
) -> V1Result<([bool; 3], Option<(Fd, Winsize)>)> {
    let mut ttys = [false; 3];
    let mut first = None;
    for fd in 0..3 {
        let ws = frame.mem.alloc_mut::<Winsize>()?;
        match frame
            .syscalls
            .ioctl(&frame.caller(), Fd(fd), IoctlRequest::TIOCGWINSZ, ws.addr())
            .await
        {
            Ok(_) => {
                ttys[fd as usize] = true;
                first.get_or_insert((Fd(fd), frame.mem.take(ws)));
            }
            Err(SysErr::Errno(_)) => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok((ttys, first))
}

async fn tcgets<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<Termios> {
    let termios = frame.mem.alloc_mut::<Termios>()?;
    frame
        .syscalls
        .ioctl(&frame.caller(), fd, IoctlRequest::TCGETS, termios.addr())
        .await?;
    Ok(frame.mem.take(termios))
}

fn modes_of(termios: &Termios) -> Modes {
    Modes {
        echo: termios.c_lflag.0 & TermiosLflag::ECHO.0 != 0,
        line_buffered: termios.c_lflag.0 & TermiosLflag::ICANON.0 != 0,
        line_feeds: termios.c_iflag.0 & TermiosIflag::IGNCR.0 != 0,
    }
}

fn set_flag(bits: &mut u32, flag: u32, on: bool) {
    if on {
        *bits |= flag;
    } else {
        *bits &= !flag;
    }
}

fn apply(termios: &mut Termios, modes: Modes) {
    set_flag(&mut termios.c_lflag.0, TermiosLflag::ECHO.0, modes.echo);
    set_flag(
        &mut termios.c_lflag.0,
        TermiosLflag::ICANON.0,
        modes.line_buffered,
    );
    set_flag(
        &mut termios.c_iflag.0,
        TermiosIflag::IGNCR.0,
        modes.line_feeds,
    );
}

/// A pixel size, or the default when the terminal does not know it.
fn or_default(value: u16, default: u32) -> u32 {
    match value {
        0 => default,
        value => u32::from(value),
    }
}

/// The v1 terminal state for `ttys`, the first terminal's window size, and `modes`.
fn tty_state(ttys: [bool; 3], winsize: Option<&Winsize>, modes: Modes) -> Tty {
    let (cols, rows, width, height) = match winsize {
        Some(ws) => (
            or_default(ws.ws_col, DEFAULT_COLS),
            or_default(ws.ws_row, DEFAULT_ROWS),
            or_default(ws.ws_xpixel, DEFAULT_WIDTH),
            or_default(ws.ws_ypixel, DEFAULT_HEIGHT),
        ),
        None => (DEFAULT_COLS, DEFAULT_ROWS, DEFAULT_WIDTH, DEFAULT_HEIGHT),
    };
    Tty {
        cols,
        rows,
        width,
        height,
        stdin_tty: bool(ttys[0]),
        stdout_tty: bool(ttys[1]),
        stderr_tty: bool(ttys[2]),
        echo: bool(modes.echo),
        line_buffered: bool(modes.line_buffered),
        line_feeds: bool(modes.line_feeds),
    }
}

pub(crate) async fn tty_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let (ttys, first) = stdio(frame).await?;
        let modes = match &first {
            Some((fd, _)) => modes_of(&tcgets(frame, *fd).await?),
            None => *frame.proc().await?.proc.tty.modes.lock().unwrap(),
        };
        let tty = tty_state(ttys, first.as_ref().map(|(_, ws)| ws), modes);
        frame.write(out as u32, &tty).await
    })
    .await
}

pub(crate) async fn tty_set<S: Syscalls + Control + 'static>(
    env: Env<S>,
    state: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let tty: Tty = frame.read(state as u32).await?;
        let modes = Modes {
            echo: tty.echo == Bool::TRUE,
            line_buffered: tty.line_buffered == Bool::TRUE,
            line_feeds: tty.line_feeds == Bool::TRUE,
        };
        let (_, first) = stdio(frame).await?;
        match first {
            Some((fd, _)) => {
                let mut termios = tcgets(frame, fd).await?;
                apply(&mut termios, modes);
                let termios = frame.mem.alloc(&termios)?;
                frame
                    .syscalls
                    .ioctl(&frame.caller(), fd, IoctlRequest::TCSETS, termios.addr())
                    .await?;
            }
            None => *frame.proc().await?.proc.tty.modes.lock().unwrap() = modes,
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use wasixv2_abi::GuestLayout;

    use super::*;

    #[test]
    fn modes_round_trip_through_termios() {
        let mut termios = Termios::ZERO;
        let modes = Modes {
            echo: true,
            line_buffered: false,
            line_feeds: true,
        };
        apply(&mut termios, modes);
        assert_eq!(modes_of(&termios), modes);
        apply(&mut termios, Modes::default());
        assert_eq!(modes_of(&termios), Modes::default());
    }

    #[test]
    fn no_terminal_is_v1_defaults() {
        let tty = tty_state([false; 3], None, Modes::default());
        assert_eq!((tty.cols, tty.rows), (80, 25));
        assert_eq!(tty.stdout_tty, Bool::FALSE);
        assert_eq!(tty.line_feeds, Bool::TRUE);
        let ws = Winsize {
            ws_row: 50,
            ws_col: 132,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let tty = tty_state([false, true, false], Some(&ws), Modes::default());
        assert_eq!((tty.cols, tty.rows, tty.width), (132, 50, 800));
        assert_eq!(tty.stdout_tty, Bool::TRUE);
    }
}
