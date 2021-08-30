//! This module contains the standard I/O streams, i.e. “emulated”
//! `stdin`, `stdout` and `stderr`.

use crate::{FileDescriptor, FsError, Result, VirtualFile};
use std::io::{self, Read, Seek, Write};

macro_rules! impl_virtualfile_on_std_streams {
    ($name:ident { readable: $readable:expr, writable: $writable:expr $(,)* }) => {
        /// A wrapper type around the standard I/O stream of the same
        /// name that implements `VirtualFile`.
        #[derive(Debug, Default)]
        pub struct $name {
            pub buf: Vec<u8>,
        }

        impl $name {
            const fn is_readable(&self) -> bool {
                $readable
            }

            const fn is_writable(&self) -> bool {
                $writable
            }
        }

        impl VirtualFile for $name {
            fn last_accessed(&self) -> u64 {
                0
            }

            fn last_modified(&self) -> u64 {
                0
            }

            fn created_time(&self) -> u64 {
                0
            }

            fn size(&self) -> u64 {
                0
            }

            fn set_len(&mut self, _new_size: u64) -> Result<()> {
                Err(FsError::PermissionDenied)
            }

            fn unlink(&mut self) -> Result<()> {
                Ok(())
            }

            fn bytes_available(&self) -> Result<usize> {
                unimplemented!();
            }

            fn get_fd(&self) -> Option<FileDescriptor> {
                None
            }
        }

        impl_virtualfile_on_std_streams!(impl Seek for $name);
        impl_virtualfile_on_std_streams!(impl Read for $name);
        impl_virtualfile_on_std_streams!(impl Write for $name);
    };

    (impl Seek for $name:ident) => {
        impl Seek for $name {
            fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    concat!("cannot seek `", stringify!($name), "`"),
                ))
            }
        }
    };

    (impl Read for $name:ident) => {
        impl Read for $name {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.is_readable() {
                    let length = self.buf.as_slice().read(buf)?;

                    // Remove what has been consumed.
                    self.buf.drain(..length);

                    Ok(length)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot read from `", stringify!($name), "`"),
                    ))
                }
            }

            fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
                if self.is_readable() {
                    let length = self.buf.as_slice().read_to_end(buf)?;

                    // Remove what has been consumed.
                    self.buf.clear();

                    Ok(length)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot read from `", stringify!($name), "`"),
                    ))
                }
            }

            fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
                if self.is_readable() {
                    let length = self.buf.as_slice().read_to_string(buf)?;

                    // Remove what has been consumed.
                    self.buf.drain(..length);

                    Ok(length)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot read from `", stringify!($name), "`"),
                    ))
                }
            }

            fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
                if self.is_readable() {
                    self.buf.as_slice().read_exact(buf)?;

                    self.buf.drain(..buf.len());

                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot read from `", stringify!($name), "`"),
                    ))
                }
            }
        }
    };

    (impl Write for $name:ident) => {
        impl Write for $name {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                if self.is_writable() {
                    self.buf.write(buf)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot write to `", stringify!($name), "`"),
                    ))
                }
            }

            fn flush(&mut self) -> io::Result<()> {
                if self.is_writable() {
                    self.buf.flush()
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot flush `", stringify!($name), "`"),
                    ))
                }
            }

            fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
                if self.is_writable() {
                    self.buf.write_all(buf)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot write to `", stringify!($name), "`"),
                    ))
                }
            }
        }
    };
}

impl_virtualfile_on_std_streams!(Stdin {
    readable: true,
    writable: false,
});
impl_virtualfile_on_std_streams!(Stdout {
    readable: false,
    writable: true,
});
impl_virtualfile_on_std_streams!(Stderr {
    readable: false,
    writable: true,
});

#[cfg(test)]
mod test_read_write_seek {
    use crate::mem_fs::*;
    use std::io::{self, Read, Seek, Write};

    #[test]
    fn test_read_stdin() {
        let mut stdin = Stdin {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };
        let mut buffer = [0; 3];

        assert!(
            matches!(stdin.read(&mut buffer), Ok(3)),
            "reading bytes from `stdin`",
        );
        assert_eq!(
            buffer,
            [b'f', b'o', b'o'],
            "checking the bytes read from `stdin`"
        );

        let mut buffer = Vec::new();

        assert!(
            matches!(stdin.read_to_end(&mut buffer), Ok(3)),
            "reading bytes again from `stdin`",
        );
        assert_eq!(
            buffer,
            &[b'b', b'a', b'r'],
            "checking the bytes read from `stdin`"
        );

        let mut buffer = [0; 1];

        assert!(
            stdin.read_exact(&mut buffer).is_err(),
            "cannot read bytes again because `stdin` has fully consumed",
        );
    }

    #[test]
    fn test_write_stdin() {
        let mut stdin = Stdin { buf: vec![] };

        assert!(stdin.write(b"bazqux").is_err(), "cannot write into `stdin`");
    }

    #[test]
    fn test_seek_stdin() {
        let mut stdin = Stdin {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };

        assert!(
            stdin.seek(io::SeekFrom::End(0)).is_err(),
            "cannot seek `stdin`",
        );
    }

    #[test]
    fn test_read_stdout() {
        let mut stdout = Stdout {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };
        let mut buffer = String::new();

        assert!(
            stdout.read_to_string(&mut buffer).is_err(),
            "cannot read from `stdout`"
        );
    }

    #[test]
    fn test_write_stdout() {
        let mut stdout = Stdout { buf: vec![] };

        assert!(
            matches!(stdout.write(b"baz"), Ok(3)),
            "writing into `stdout`",
        );
        assert!(
            matches!(stdout.write(b"qux"), Ok(3)),
            "writing again into `stdout`",
        );
        assert_eq!(
            stdout.buf,
            &[b'b', b'a', b'z', b'q', b'u', b'x'],
            "checking the content of `stdout`",
        );
    }

    #[test]
    fn test_seek_stdout() {
        let mut stdout = Stdout {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };

        assert!(
            stdout.seek(io::SeekFrom::End(0)).is_err(),
            "cannot seek `stdout`",
        );
    }

    #[test]
    fn test_read_stderr() {
        let mut stderr = Stderr {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };
        let mut buffer = String::new();

        assert!(
            stderr.read_to_string(&mut buffer).is_err(),
            "cannot read from `stderr`"
        );
    }

    #[test]
    fn test_write_stderr() {
        let mut stderr = Stderr { buf: vec![] };

        assert!(
            matches!(stderr.write(b"baz"), Ok(3)),
            "writing into `stderr`",
        );
        assert!(
            matches!(stderr.write(b"qux"), Ok(3)),
            "writing again into `stderr`",
        );
        assert_eq!(
            stderr.buf,
            &[b'b', b'a', b'z', b'q', b'u', b'x'],
            "checking the content of `stderr`",
        );
    }

    #[test]
    fn test_seek_stderr() {
        let mut stderr = Stderr {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };

        assert!(
            stderr.seek(io::SeekFrom::End(0)).is_err(),
            "cannot seek `stderr`",
        );
    }
}
