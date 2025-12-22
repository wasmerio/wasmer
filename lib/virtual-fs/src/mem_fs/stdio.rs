//! This module contains the standard I/O streams, i.e. “emulated”
//! `stdin`, `stdout` and `stderr`.

use crate::{FsError, Result, VirtualFile};
use std::io::{self, Write};

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

        #[async_trait::async_trait]
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

            fn set_len(& mut self, _new_size: u64) -> Result<()> {
                Err(FsError::PermissionDenied)
            }

            fn unlink(&mut self) -> Result<()> {
                Ok(())
            }

            fn poll_read_ready(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<usize>> {
                std::task::Poll::Ready(Ok(self.buf.len()))
            }

            fn poll_write_ready(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<usize>> {
                std::task::Poll::Ready(Ok(8192))
            }
        }

        impl_virtualfile_on_std_streams!(impl AsyncSeek for $name);
        impl_virtualfile_on_std_streams!(impl AsyncRead for $name);
        impl_virtualfile_on_std_streams!(impl AsyncWrite for $name);
    };

    (impl AsyncSeek for $name:ident) => {
        impl tokio::io::AsyncSeek for $name {
            fn start_seek(
                self: std::pin::Pin<&mut Self>,
                _position: io::SeekFrom
            ) -> io::Result<()> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    concat!("cannot seek `", stringify!($name), "`"),
                ))
            }
            fn poll_complete(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>
            ) -> std::task::Poll<io::Result<u64>>
            {
                std::task::Poll::Ready(
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        concat!("cannot seek `", stringify!($name), "`"),
                    ))
                )
            }
        }
    };

    (impl AsyncRead for $name:ident) => {
        impl tokio::io::AsyncRead for $name {
            fn poll_read(
                mut self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<io::Result<()>> {
                std::task::Poll::Ready(
                    if self.is_readable() {
                        let length = buf.remaining().min(self.buf.len());
                        buf.put_slice(&self.buf[..length]);

                        // Remove what has been consumed.
                        self.buf.drain(..length);

                        Ok(())
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            concat!("cannot read from `", stringify!($name), "`"),
                        ))
                    }
                )
            }
        }
    };

    (impl AsyncWrite for $name:ident) => {
        impl tokio::io::AsyncWrite for $name {
            fn poll_write(
                mut self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
                buf: &[u8],
            ) -> std::task::Poll<io::Result<usize>> {
                std::task::Poll::Ready(
                    if self.is_writable() {
                        self.buf.write(buf)
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            concat!("cannot write to `", stringify!($name), "`"),
                        ))
                    }
                )
            }

            fn poll_flush(
                mut self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>
            ) -> std::task::Poll<io::Result<()>> {
                std::task::Poll::Ready(
                    if self.is_writable() {
                        self.buf.flush()
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            concat!("cannot flush `", stringify!($name), "`"),
                        ))
                    }
                )
            }

            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>
            ) -> std::task::Poll<io::Result<()>> {
                std::task::Poll::Ready(Ok(()))
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
    use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

    use crate::mem_fs::*;
    use std::io::{self};

    #[tokio::test]
    async fn test_read_stdin() {
        let mut stdin = Stdin {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };
        let mut buffer = [0; 3];

        assert!(
            matches!(stdin.read(&mut buffer).await, Ok(3)),
            "reading bytes from `stdin`",
        );
        assert_eq!(
            buffer,
            [b'f', b'o', b'o'],
            "checking the bytes read from `stdin`"
        );

        let mut buffer = Vec::new();

        assert!(
            matches!(stdin.read_to_end(&mut buffer).await, Ok(3)),
            "reading bytes again from `stdin`",
        );
        assert_eq!(buffer, b"bar", "checking the bytes read from `stdin`");

        let mut buffer = [0; 1];

        assert!(
            stdin.read_exact(&mut buffer).await.is_err(),
            "cannot read bytes again because `stdin` has fully consumed",
        );
    }

    #[tokio::test]
    async fn test_write_stdin() {
        let mut stdin = Stdin { buf: vec![] };

        assert!(
            stdin.write(b"bazqux").await.is_err(),
            "cannot write into `stdin`"
        );
    }

    #[tokio::test]
    async fn test_seek_stdin() {
        let mut stdin = Stdin {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };

        assert!(
            stdin.seek(io::SeekFrom::End(0)).await.is_err(),
            "cannot seek `stdin`",
        );
    }

    #[tokio::test]
    async fn test_read_stdout() {
        let mut stdout = Stdout {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };
        let mut buffer = String::new();

        assert!(
            stdout.read_to_string(&mut buffer).await.is_err(),
            "cannot read from `stdout`"
        );
    }

    #[tokio::test]
    async fn test_write_stdout() {
        let mut stdout = Stdout { buf: vec![] };

        assert!(
            matches!(stdout.write(b"baz").await, Ok(3)),
            "writing into `stdout`",
        );
        assert!(
            matches!(stdout.write(b"qux").await, Ok(3)),
            "writing again into `stdout`",
        );
        assert_eq!(stdout.buf, b"bazqux", "checking the content of `stdout`");
    }

    #[tokio::test]
    async fn test_seek_stdout() {
        let mut stdout = Stdout {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };

        assert!(
            stdout.seek(io::SeekFrom::End(0)).await.is_err(),
            "cannot seek `stdout`",
        );
    }

    #[tokio::test]
    async fn test_read_stderr() {
        let mut stderr = Stderr {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };
        let mut buffer = String::new();

        assert!(
            stderr.read_to_string(&mut buffer).await.is_err(),
            "cannot read from `stderr`"
        );
    }

    #[tokio::test]
    async fn test_write_stderr() {
        let mut stderr = Stderr { buf: vec![] };

        assert!(
            matches!(stderr.write(b"baz").await, Ok(3)),
            "writing into `stderr`",
        );
        assert!(
            matches!(stderr.write(b"qux").await, Ok(3)),
            "writing again into `stderr`",
        );
        assert_eq!(stderr.buf, b"bazqux", "checking the content of `stderr`");
    }

    #[tokio::test]
    async fn test_seek_stderr() {
        let mut stderr = Stderr {
            buf: vec![b'f', b'o', b'o', b'b', b'a', b'r'],
        };

        assert!(
            stderr.seek(io::SeekFrom::End(0)).await.is_err(),
            "cannot seek `stderr`",
        );
    }
}
