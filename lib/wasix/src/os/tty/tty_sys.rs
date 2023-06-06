use super::TtyBridge;
use crate::WasiTtyState;

/// [`TtyBridge`] implementation for Unix systems.
#[derive(Debug, Default, Clone)]
pub struct SysTty;

impl TtyBridge for SysTty {
    fn reset(&self) {
        sys::reset().ok();
    }

    fn tty_get(&self) -> WasiTtyState {
        let echo = sys::is_mode_echo();
        let line_buffered = sys::is_mode_line_buffering();
        let line_feeds = sys::is_mode_line_feeds();
        let stdin_tty = sys::is_stdin_tty();
        let stdout_tty = sys::is_stdout_tty();
        let stderr_tty = sys::is_stderr_tty();

        if let Some((w, h)) = term_size::dimensions() {
            WasiTtyState {
                cols: w as u32,
                rows: h as u32,
                width: 800,
                height: 600,
                stdin_tty,
                stdout_tty,
                stderr_tty,
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
                stdin_tty,
                stdout_tty,
                stderr_tty,
                echo,
                line_buffered,
                line_feeds,
            }
        }
    }

    fn tty_set(&self, tty_state: WasiTtyState) {
        if tty_state.echo {
            sys::set_mode_echo().ok();
        } else {
            sys::set_mode_no_echo().ok();
        }
        if tty_state.line_buffered {
            sys::set_mode_line_buffered().ok();
        } else {
            sys::set_mode_no_line_buffered().ok();
        }
        if tty_state.line_feeds {
            sys::set_mode_line_feeds().ok();
        } else {
            sys::set_mode_no_line_feeds().ok();
        }
    }
}

#[allow(unused_mut)]
#[cfg(all(unix, not(target_os = "ios")))]
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

    fn io_result(ret: libc::c_int) -> std::io::Result<()> {
        match ret {
            0 => Ok(()),
            _ => Err(std::io::Error::last_os_error()),
        }
    }

    pub fn reset() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ISIG | ICANON | IEXTEN | ECHO | ECHOE | ECHOK | ECHOCTL;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
        Ok(())
    }

    pub fn is_stdin_tty() -> bool {
        ::termios::Termios::from_fd(0).is_ok()
    }

    pub fn is_stdout_tty() -> bool {
        ::termios::Termios::from_fd(1).is_ok()
    }

    pub fn is_stderr_tty() -> bool {
        ::termios::Termios::from_fd(2).is_ok()
    }

    pub fn is_mode_echo() -> bool {
        if let Ok(termios) = ::termios::Termios::from_fd(0) {
            (termios.c_lflag & ::termios::ECHO) != 0
        } else {
            false
        }
    }

    pub fn is_mode_line_buffering() -> bool {
        if let Ok(termios) = ::termios::Termios::from_fd(0) {
            (termios.c_lflag & ::termios::ICANON) != 0
        } else {
            false
        }
    }

    pub fn is_mode_line_feeds() -> bool {
        if let Ok(termios) = ::termios::Termios::from_fd(0) {
            (termios.c_lflag & ::termios::ONLCR) != 0
        } else {
            false
        }
    }

    pub fn set_mode_no_echo() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
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
        Ok(())
    }

    pub fn set_mode_echo() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
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
        Ok(())
    }

    pub fn set_mode_no_line_buffered() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag &= !ICANON;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
        Ok(())
    }

    pub fn set_mode_line_buffered() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ICANON;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
        Ok(())
    }

    pub fn set_mode_no_line_feeds() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag &= !ONLCR;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
        Ok(())
    }

    pub fn set_mode_line_feeds() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ONLCR;

        unsafe { tcsetattr(0, TCSANOW, &termios) };
        Ok(())
    }
}

#[cfg(any(not(unix), target_os = "ios"))]
mod sys {
    pub fn reset() -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn is_stdin_tty() -> bool {
        false
    }

    pub fn is_stdout_tty() -> bool {
        false
    }

    pub fn is_stderr_tty() -> bool {
        false
    }

    pub fn is_mode_echo() -> bool {
        true
    }

    pub fn is_mode_line_buffering() -> bool {
        true
    }

    pub fn is_mode_line_feeds() -> bool {
        true
    }

    pub fn set_mode_no_echo() -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn set_mode_echo() -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn set_mode_no_line_buffered() -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn set_mode_line_buffered() -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn set_mode_no_line_feeds() -> Result<(), anyhow::Error> {
        Ok(())
    }

    pub fn set_mode_line_feeds() -> Result<(), anyhow::Error> {
        Ok(())
    }
}
