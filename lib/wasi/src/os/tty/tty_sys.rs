use super::TtyBridge;
use crate::WasiTtyState;

/// [`TtyBridge`] implementation for Unix systems.
pub struct SysTyy;

impl TtyBridge for SysTyy {
    fn tty_get(&self) -> WasiTtyState {
        let mut echo = false;
        let mut line_buffered = false;
        let mut line_feeds = false;

        if let Ok(termios) = termios::Termios::from_fd(0) {
            echo = (termios.c_lflag & termios::ECHO) != 0;
            line_buffered = (termios.c_lflag & termios::ICANON) != 0;
            line_feeds = (termios.c_lflag & termios::ONLCR) != 0;
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
        #[cfg(unix)]
        {
            if tty_state.echo {
                unix::set_mode_echo();
            } else {
                unix::set_mode_no_echo();
            }
            if tty_state.line_buffered {
                unix::set_mode_line_buffered();
            } else {
                unix::set_mode_no_line_buffered();
            }
            if tty_state.line_feeds {
                unix::set_mode_line_feeds();
            } else {
                unix::set_mode_no_line_feeds();
            }
        }
    }
}

#[cfg(unix)]
mod unix {
    #![allow(unused_imports)]
    use {
        libc::{
            c_int, tcsetattr, termios, ECHO, ECHOE, ECHONL, ICANON, ICRNL, IEXTEN, ISIG, IXON,
            OPOST, TCSANOW,
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
    pub fn set_mode_no_echo() -> std::fs::File {
        let tty = std::fs::File::open("/dev/tty").unwrap();
        let fd = tty.as_raw_fd();

        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(fd, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag &= !ECHO;
        termios.c_lflag &= !ECHOE;
        termios.c_lflag &= !ISIG;
        termios.c_lflag &= !IXON;
        termios.c_lflag &= !IEXTEN;
        termios.c_lflag &= !ICRNL;
        termios.c_lflag &= !OPOST;

        unsafe { tcsetattr(fd, TCSANOW, &termios) };
        tty
    }

    #[cfg(unix)]
    pub fn set_mode_echo() -> std::fs::File {
        let tty = std::fs::File::open("/dev/tty").unwrap();
        let fd = tty.as_raw_fd();

        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(fd, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ECHO;
        termios.c_lflag |= ECHOE;
        termios.c_lflag |= ISIG;
        termios.c_lflag |= IXON;
        termios.c_lflag |= IEXTEN;
        termios.c_lflag |= ICRNL;
        termios.c_lflag |= OPOST;

        unsafe { tcsetattr(fd, TCSANOW, &termios) };
        tty
    }

    #[cfg(unix)]
    pub fn set_mode_no_line_feeds() -> std::fs::File {
        let tty = std::fs::File::open("/dev/tty").unwrap();
        let fd = tty.as_raw_fd();

        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(fd, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag &= !ICANON;

        unsafe { tcsetattr(fd, TCSANOW, &termios) };
        tty
    }

    #[cfg(unix)]
    pub fn set_mode_line_feeds() -> std::fs::File {
        let tty = std::fs::File::open("/dev/tty").unwrap();
        let fd = tty.as_raw_fd();

        let mut termios = mem::MaybeUninit::<termios>::uninit();
        io_result(unsafe { ::libc::tcgetattr(fd, termios.as_mut_ptr()) }).unwrap();
        let mut termios = unsafe { termios.assume_init() };

        termios.c_lflag |= ICANON;

        unsafe { tcsetattr(fd, TCSANOW, &termios) };
        tty
    }

    // #[cfg(unix)]
    // pub fn set_mode_no_line_feeds() -> std::fs::File {
    //     let tty = std::fs::File::open("/dev/tty").unwrap();
    //     let fd = tty.as_raw_fd();

    //     let mut termios = mem::MaybeUninit::<termios>::uninit();
    //     io_result(unsafe { ::libc::tcgetattr(fd, termios.as_mut_ptr()) }).unwrap();
    //     let mut termios = unsafe { termios.assume_init() };

    //     termios.c_lflag &= !::termios::ONLCR;

    //     unsafe { tcsetattr(fd, TCSANOW, &termios) };
    //     tty
    // }

    // #[cfg(unix)]
    // pub fn set_mode_line_feeds() -> std::fs::File {
    //     let tty = std::fs::File::open("/dev/tty").unwrap();
    //     let fd = tty.as_raw_fd();

    //     let mut termios = mem::MaybeUninit::<termios>::uninit();
    //     io_result(unsafe { ::libc::tcgetattr(fd, termios.as_mut_ptr()) }).unwrap();
    //     let mut termios = unsafe { termios.assume_init() };

    //     termios.c_lflag |= ONLCR;

    //     unsafe { tcsetattr(fd, TCSANOW, &termios) };
    //     tty
    // }
}
