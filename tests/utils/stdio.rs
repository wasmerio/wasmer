use super::file_descriptor::FileDescriptor;
use libc;
use std::io;
use std::io::BufReader;
use std::io::Read;

// A struct to hold the references to the base stdout and the captured one
pub struct StdioCapturer {
    stdout_backup: libc::c_int,
    stderr_backup: libc::c_int,
    stdout_reader: libc::c_int,
    stderr_reader: libc::c_int,
}

#[cfg(not(target_os = "windows"))]
use libc::{STDERR_FILENO, STDOUT_FILENO};

#[cfg(target_os = "windows")]
const _STDIN_FILENO: libc::c_int = 0;
#[cfg(target_os = "windows")]
const STDOUT_FILENO: libc::c_int = 1;
#[cfg(target_os = "windows")]
const STDERR_FILENO: libc::c_int = 2;

fn pipe(ptr: *mut libc::c_int) -> Result<libc::c_int, io::Error> {
    #[cfg(not(target_os = "windows"))]
    let result = unsafe { libc::pipe(ptr) };
    #[cfg(target_os = "windows")]
    let result = unsafe { libc::pipe(ptr, 1000, libc::O_TEXT) };

    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// `dup` creates a new `fd`, make sure you `close` it when you're done with it!!
fn dup(oldfd: libc::c_int) -> Result<libc::c_int, io::Error> {
    let result = unsafe { libc::dup(oldfd) };

    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

fn dup2(oldfd: libc::c_int, newfd: libc::c_int) -> Result<libc::c_int, io::Error> {
    let result = unsafe { libc::dup2(oldfd, newfd) };

    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

fn close(fd: libc::c_int) -> Result<libc::c_int, io::Error> {
    let result = unsafe { libc::close(fd) };

    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

// Implementation inspired in
// https://github.com/rust-lang/rust/blob/7d52cbce6db83e4fc2d8706b4e4b9c7da76cbcf8/src/test/run-pass/issues/issue-30490.rs
// Currently only works in Unix systems (Mac, Linux)
impl StdioCapturer {
    fn pipe() -> (libc::c_int, libc::c_int) {
        let mut fds = [0; 2];
        pipe(fds.as_mut_ptr()).unwrap();

        (fds[0], fds[1])
    }

    pub fn new() -> Self {
        let stdout_backup = dup(STDOUT_FILENO).unwrap();
        let stderr_backup = dup(STDERR_FILENO).unwrap();

        let (stdout_reader, stdout_writer) = Self::pipe();
        let (stderr_reader, stderr_writer) = Self::pipe();

        dup2(stdout_writer, STDOUT_FILENO).unwrap();
        dup2(stderr_writer, STDERR_FILENO).unwrap();

        // Make sure we close any duplicates of the writer end of the pipe,
        // otherwise we can get stuck reading from the pipe which has open
        // writers but no one supplying any input
        close(stdout_writer).unwrap();
        close(stderr_writer).unwrap();

        StdioCapturer {
            stdout_backup,
            stderr_backup,
            stdout_reader,
            stderr_reader,
        }
    }

    pub fn end(self) -> Result<(String, String), std::io::Error> {
        dup2(self.stdout_backup, STDOUT_FILENO).unwrap();
        dup2(self.stderr_backup, STDERR_FILENO).unwrap();

        close(self.stdout_backup).unwrap();
        close(self.stderr_backup).unwrap();

        let fd = FileDescriptor::new(self.stdout_reader);
        let mut reader = BufReader::new(fd);
        let mut stdout_read = "".to_string();
        let _ = reader.read_to_string(&mut stdout_read)?;

        let fd = FileDescriptor::new(self.stderr_reader);
        let mut reader = BufReader::new(fd);
        let mut stderr_read = "".to_string();
        let _ = reader.read_to_string(&mut stderr_read)?;

        Ok((stdout_read, stderr_read))
    }
}
