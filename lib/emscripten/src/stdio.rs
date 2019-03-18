use super::file_descriptor::FileDescriptor;
use libc;
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

// Implementation inspired in
// https://github.com/rust-lang/rust/blob/7d52cbce6db83e4fc2d8706b4e4b9c7da76cbcf8/src/test/run-pass/issues/issue-30490.rs
// Currently only works in Unix systems (Mac, Linux)
impl StdioCapturer {
    fn pipe() -> (libc::c_int, libc::c_int) {
        let mut fds = [0; 2];

        #[cfg(not(target_os = "windows"))]
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        #[cfg(target_os = "windows")]
        assert_eq!(
            unsafe { libc::pipe(fds.as_mut_ptr(), 1000, libc::O_TEXT) },
            0
        );

        (fds[0], fds[1])
    }

    pub fn new() -> Self {
        let stdout_backup = unsafe { libc::dup(STDOUT_FILENO) };
        let stderr_backup = unsafe { libc::dup(STDERR_FILENO) };

        let (stdout_reader, stdout_writer) = Self::pipe();
        let (stderr_reader, stderr_writer) = Self::pipe();

        assert!(unsafe { libc::dup2(stdout_writer, STDOUT_FILENO) } > -1);
        assert!(unsafe { libc::dup2(stderr_writer, STDERR_FILENO) } > -1);

        // Make sure we close any duplicates of the writer end of the pipe,
        // otherwise we can get stuck reading from the pipe which has open
        // writers but no one supplying any input
        assert_eq!(unsafe { libc::close(stdout_writer) }, 0);
        assert_eq!(unsafe { libc::close(stderr_writer) }, 0);

        StdioCapturer {
            stdout_backup,
            stderr_backup,
            stdout_reader,
            stderr_reader,
        }
    }

    pub fn end(self) -> Result<(String, String), std::io::Error> {
        // The Stdio passed into the Command took over (and closed) std{out, err}
        // so we should restore them as they were.

        assert!(unsafe { libc::dup2(self.stdout_backup, STDOUT_FILENO) } > -1);
        assert!(unsafe { libc::dup2(self.stderr_backup, STDERR_FILENO) } > -1);

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
