use libc;
use std::fs::File;
use std::io::Read;
use std::os::unix::io::FromRawFd;

// A struct to hold the references to the base stdout and the captured one
pub struct StdioCapturer {
    stdout_backup: libc::c_int,
    stderr_backup: libc::c_int,
    stdout_reader: libc::c_int,
    stderr_reader: libc::c_int,
}

// Implementation inspired in
// https://github.com/rust-lang/rust/blob/7d52cbce6db83e4fc2d8706b4e4b9c7da76cbcf8/src/test/run-pass/issues/issue-30490.rs
// Currently only works in Unix systems (Mac, Linux)
impl StdioCapturer {
    fn pipe() -> (libc::c_int, libc::c_int) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        (fds[0], fds[1])
    }

    pub fn new() -> Self {
        let stdout_backup = unsafe { libc::dup(libc::STDOUT_FILENO) };
        let stderr_backup = unsafe { libc::dup(libc::STDERR_FILENO) };

        let (stdout_reader, stdout_writer) = Self::pipe();
        let (stderr_reader, stderr_writer) = Self::pipe();

        // std::io::stdout().flush().unwrap();
        // std::io::stderr().flush().unwrap();

        assert!(unsafe { libc::dup2(stdout_writer, libc::STDOUT_FILENO) } > -1);
        assert!(unsafe { libc::dup2(stderr_writer, libc::STDERR_FILENO) } > -1);

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

    pub fn end(self) -> (String, String) {
        // The Stdio passed into the Command took over (and closed) std{out, err}
        // so we should restore them as they were.

        assert!(unsafe { libc::dup2(self.stdout_backup, libc::STDOUT_FILENO) } > -1);
        assert!(unsafe { libc::dup2(self.stderr_backup, libc::STDERR_FILENO) } > -1);

        // assert_eq!(unsafe { libc::close(self.stdout_backup) }, 0);
        // assert_eq!(unsafe { libc::close(self.stderr_backup) }, 0);

        let mut stdout_read = String::new();
        let mut stdout_file: File = unsafe { FromRawFd::from_raw_fd(self.stdout_reader) };
        stdout_file
            .read_to_string(&mut stdout_read)
            .expect("failed to read from stdout file");

        let mut stderr_read = String::new();
        let mut stderr_file: File = unsafe { FromRawFd::from_raw_fd(self.stderr_reader) };
        stderr_file
            .read_to_string(&mut stderr_read)
            .expect("failed to read from stdout file");

        (stdout_read, stderr_read)
    }
}
