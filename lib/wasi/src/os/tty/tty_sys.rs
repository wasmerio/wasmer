use super::TtyBridge;
use crate::WasiTtyState;

/// [`TtyBridge`] implementation for Unix systems.
#[derive(Debug, Default, Clone)]
pub struct SysTyy;

impl TtyBridge for SysTyy {
    fn reset(&self) {
        sys::reset();
    }

    #[allow(unused_assignments)]
    fn tty_get(&self) -> WasiTtyState {
        let mut echo = false;
        let mut line_buffered = false;
        let mut line_feeds = false;

        #[cfg(unix)]
        {
            echo = sys::is_mode_echo();
            line_buffered = sys::is_mode_line_buffering();
            line_feeds = sys::is_mode_line_feeds();
        }

        if let Some((w, h)) = term_size::dimensions() {
            WasiTtyState {
                cols: w as u32,
                rows: h as u32,
                width: 800,
                height: 600,
                stdin_tty: true,
                stdout_tty: true,
                stderr_tty: true,
                echo,
                line_buffered,
                line_feeds,
            }
        } else {
            WasiTtyState {
                rows: 80,
                cols: 25,
                width: 800,
                height: 600,
                stdin_tty: true,
                stdout_tty: true,
                stderr_tty: true,
                echo,
                line_buffered,
                line_feeds,
            }
        }
    }

    fn tty_set(&self, tty_state: WasiTtyState) {
        if tty_state.echo {
            sys::set_mode_echo();
        } else {
            sys::set_mode_no_echo();
        }
        if tty_state.line_buffered {
            sys::set_mode_line_buffered();
        } else {
            sys::set_mode_no_line_buffered();
        }
        if tty_state.line_feeds {
            sys::set_mode_line_feeds();
        } else {
            sys::set_mode_no_line_feeds();
        }
    }
}

#[allow(unused_mut)]
#[cfg(unix)]
mod sys {
    #![allow(unused_imports)]
    use {
        libc::{
            c_int, tcsetattr, termios, ECHO, ECHOCTL, ECHOE, ECHOK, ECHONL, ICANON, ICRNL, IEXTEN,
            IGNCR, ISIG, IXON, ONLCR, OPOST, TCSANOW,
        },
        std::mem,
        std::os::unix::io::AsRawFd,
    };

    #[cfg(unix)]
    pub fn io_result(ret: libc::c_int) -> std::io::Result<()> {
        match ret {
            0 => Ok(()),
            _ => Err(std::io::Error::last_os_error()),
        }
    }

    #[cfg(unix)]
    pub fn reset() {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ISIG | ICANON | IEXTEN | ECHO | ECHOE | ECHOK | ECHOCTL;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
    }

    #[cfg(unix)]
    pub fn is_mode_echo() -> bool {
        if let Ok(termios) = ::termios::Termios::from_fd(0) {
            (termios.c_lflag & ::termios::ECHO) != 0
        } else {
            false
        }
    }

    #[cfg(unix)]
    pub fn is_mode_line_buffering() -> bool {
        if let Ok(termios) = ::termios::Termios::from_fd(0) {
            (termios.c_lflag & ::termios::ICANON) != 0
        } else {
            false
        }
    }

    #[cfg(unix)]
    pub fn is_mode_line_feeds() -> bool {
        if let Ok(termios) = ::termios::Termios::from_fd(0) {
            (termios.c_lflag & ::termios::ONLCR) != 0
        } else {
            false
        }
    }

    #[cfg(unix)]
    pub fn set_mode_no_echo() {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag &= !ECHO;
        termios.c_lflag &= !ECHOE;
        termios.c_lflag &= !ECHOK;
        termios.c_lflag &= !ECHOCTL;
        termios.c_lflag &= !IEXTEN;
        /*
        termios.c_lflag &= !ISIG;
        termios.c_lflag &= !IXON;
        termios.c_lflag &= !ICRNL;
        termios.c_lflag &= !OPOST;
        */

        unsafe { tcsetattr(0, TCSANOW, &termios) };
    }

    #[cfg(unix)]
    pub fn set_mode_echo() {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ECHO;
        termios.c_lflag |= ECHOE;
        termios.c_lflag |= ECHOK;
        termios.c_lflag |= ECHOCTL;
        termios.c_lflag |= IEXTEN;
        /*
        termios.c_lflag |= ISIG;
        termios.c_lflag |= IXON;
        termios.c_lflag |= ICRNL;
        termios.c_lflag |= OPOST;
        */

        unsafe { tcsetattr(0, TCSANOW, &termios) };
    }

    #[cfg(unix)]
    pub fn set_mode_no_line_buffered() {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag &= !ICANON;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
    }

    #[cfg(unix)]
    pub fn set_mode_line_buffered() {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ICANON;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
    }

    #[cfg(unix)]
    pub fn set_mode_no_line_feeds() {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag &= !ONLCR;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
    }

    #[cfg(unix)]
    pub fn set_mode_line_feeds() {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ONLCR;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
    }
}
