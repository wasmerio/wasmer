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
        let (cols, rows) = sys_terminal_size::get_terminal_size();

        WasiTtyState {
            cols,
            rows,
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

mod sys_terminal_size {
    static DEFAULT_SIZE: (u32, u32) = (80, 25);

    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_terminal_size() -> (u32, u32) {
        if let Some((terminal_size::Width(width), terminal_size::Height(height))) =
            terminal_size::terminal_size()
        {
            (width.into(), height.into())
        } else {
            DEFAULT_SIZE
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn get_terminal_size() -> (u32, u32) {
        DEFAULT_SIZE
    }
}

#[allow(unused_mut, unused_imports)]
#[cfg(all(unix, not(target_os = "ios")))]
mod sys {
    use {
        libc::{
            ECHO, ECHOCTL, ECHOE, ECHOK, ECHONL, ICANON, ICRNL, IEXTEN, IGNCR, INLCR, ISIG, IXON,
            ONLCR, OPOST, TCSANOW, c_int, tcsetattr, termios,
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

        termios.c_lflag |= ISIG | IEXTEN | ECHO | ECHOE | ECHOK | ECHOCTL;
        set_line_buffering(&mut termios, true);

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

        set_line_buffering(&mut termios, false);

        unsafe { tcsetattr(0, TCSANOW, &termios) };
        Ok(())
    }

    pub fn set_mode_line_buffered() -> Result<(), anyhow::Error> {
        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(0, termios.as_mut_ptr()) })?;
        let mut termios = unsafe { termios.assume_init() };

        set_line_buffering(&mut termios, true);

        unsafe { tcsetattr(0, TCSANOW, &termios) };
        Ok(())
    }

    fn set_line_buffering(termios: &mut termios, enabled: bool) {
        if enabled {
            termios.c_lflag |= ICANON;
            termios.c_iflag |= ICRNL;
            termios.c_iflag &= !(INLCR | IGNCR);
        } else {
            termios.c_lflag &= !ICANON;
            // Preserve carriage returns so applications can distinguish Enter (CR)
            // from line feed, which is commonly used for Shift+Enter.
            termios.c_iflag &= !(ICRNL | INLCR | IGNCR);
        }
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

    #[cfg(test)]
    mod tests {
        use super::*;

        fn blank_termios() -> termios {
            // SAFETY: libc::termios is a plain C data structure for which an all-zero
            // value is valid; the tests only inspect and update its flag fields.
            unsafe { mem::zeroed() }
        }

        #[test]
        fn noncanonical_input_preserves_carriage_returns_and_line_feeds() {
            let mut state = blank_termios();
            state.c_lflag = ICANON | ECHO | ISIG;
            state.c_iflag = ICRNL | INLCR | IGNCR | IXON;
            state.c_oflag = OPOST;

            set_line_buffering(&mut state, false);

            assert_eq!(state.c_lflag & ICANON, 0);
            assert_eq!(state.c_iflag & (ICRNL | INLCR | IGNCR), 0);
            assert_ne!(state.c_lflag & ECHO, 0);
            assert_ne!(state.c_lflag & ISIG, 0);
            assert_ne!(state.c_iflag & IXON, 0);
            assert_ne!(state.c_oflag & OPOST, 0);
        }

        #[test]
        fn cooked_input_translates_carriage_returns_to_newlines() {
            let mut state = blank_termios();
            state.c_iflag = INLCR | IGNCR | IXON;

            set_line_buffering(&mut state, true);

            assert_ne!(state.c_lflag & ICANON, 0);
            assert_ne!(state.c_iflag & ICRNL, 0);
            assert_eq!(state.c_iflag & (INLCR | IGNCR), 0);
            assert_ne!(state.c_iflag & IXON, 0);
        }

        fn read_exact_with_timeout(fd: c_int, output: &mut [u8]) {
            let mut offset = 0;
            while offset < output.len() {
                let mut descriptor = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                assert_eq!(
                    unsafe { libc::poll(&mut descriptor, 1, 1_000) },
                    1,
                    "timed out waiting for PTY input"
                );
                assert_ne!(descriptor.revents & libc::POLLIN, 0);

                let read = unsafe {
                    libc::read(
                        fd,
                        output[offset..].as_mut_ptr().cast(),
                        output.len() - offset,
                    )
                };
                assert!(read > 0, "failed to read PTY input");
                offset += read as usize;
            }
        }

        #[test]
        fn noncanonical_pty_input_distinguishes_enter_from_line_feed() {
            let mut master = -1;
            let mut slave = -1;
            assert_eq!(
                unsafe {
                    libc::openpty(
                        &mut master,
                        &mut slave,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                },
                0
            );

            struct Pty {
                master: c_int,
                slave: c_int,
            }

            impl Drop for Pty {
                fn drop(&mut self) {
                    unsafe {
                        libc::close(self.master);
                        libc::close(self.slave);
                    }
                }
            }

            let pty = Pty { master, slave };
            let mut state = blank_termios();
            assert_eq!(unsafe { libc::tcgetattr(pty.slave, &mut state) }, 0);
            state.c_iflag |= ICRNL | INLCR | IGNCR;
            set_line_buffering(&mut state, false);
            assert_eq!(unsafe { libc::tcsetattr(pty.slave, TCSANOW, &state) }, 0);

            let input = [b'\r', b'\n'];
            assert_eq!(
                unsafe { libc::write(pty.master, input.as_ptr().cast(), input.len()) },
                input.len() as isize
            );

            let mut output = [0_u8; 2];
            read_exact_with_timeout(pty.slave, &mut output);
            assert_eq!(output, input);

            assert_eq!(unsafe { libc::tcgetattr(pty.slave, &mut state) }, 0);
            set_line_buffering(&mut state, true);
            assert_eq!(unsafe { libc::tcsetattr(pty.slave, TCSANOW, &state) }, 0);
            let input = [b'\r'];
            assert_eq!(
                unsafe { libc::write(pty.master, input.as_ptr().cast(), input.len()) },
                1
            );
            let mut output = [0_u8];
            read_exact_with_timeout(pty.slave, &mut output);
            assert_eq!(output, [b'\n']);
        }
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
