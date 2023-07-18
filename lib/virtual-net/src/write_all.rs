use bytes::Buf;
use bytes::Bytes;
use pin_project_lite::pin_project;
use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

pin_project! {
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct WriteAll {
        writer: Arc<Mutex<Pin<Box<dyn AsyncWrite + Send>>>>,
        bufs: VecDeque<Bytes>,
    }
}

pub(crate) fn locking_write_all(
    writer: &Arc<Mutex<Pin<Box<dyn AsyncWrite + Send>>>>,
    buf: Bytes,
) -> WriteAll {
    WriteAll {
        writer: writer.clone(),
        bufs: vec![buf].into(),
    }
}

pub(crate) fn locking_write_all_many<I>(
    writer: &Arc<Mutex<Pin<Box<dyn AsyncWrite + Send>>>>,
    bufs: I,
) -> WriteAll
where
    I: IntoIterator<Item = Bytes>,
{
    WriteAll {
        writer: writer.clone(),
        bufs: bufs.into_iter().collect(),
    }
}

impl Future for WriteAll {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = self.project();

        let mut writer = me.writer.lock().unwrap();
        while let Some(first) = me.bufs.front_mut() {
            match writer.as_mut().poll_write(cx, &first) {
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()))
                }
                Poll::Ready(Ok(amt)) => {
                    first.advance(amt);
                    if first.is_empty() {
                        me.bufs.pop_front();
                        continue;
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}
