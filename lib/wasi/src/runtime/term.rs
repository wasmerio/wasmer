#![allow(unused_imports)]
#[cfg(unix)]
use {
    libc::{
        c_int, tcsetattr, termios, ECHO, ECHOE, ECHONL, ICANON, ICRNL, IEXTEN, ISIG, IXON, OPOST, TCSANOW,
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

#[cfg(unix)]
pub fn set_mode_no_line_feeds() -> std::fs::File {
    let tty = std::fs::File::open("/dev/tty").unwrap();
    let fd = tty.as_raw_fd();

    let mut termios = mem::MaybeUninit::<termios>::uninit();
    io_result(unsafe { ::libc::tcgetattr(fd, termios.as_mut_ptr()) }).unwrap();
    let mut termios = unsafe { termios.assume_init() };

    termios.c_lflag &= !ONLCR;
    
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

    termios.c_lflag |= ONLCR;
    
    unsafe { tcsetattr(fd, TCSANOW, &termios) };
    tty
}
