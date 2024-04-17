use crate::{
    net_error_into_io_err, InterestHandler, NetworkError, SocketStatus, VirtualConnectedSocket,
    VirtualIoSource, VirtualSocket, VirtualTcpSocket,
};
use bytes::{Buf, Bytes};
use futures_util::Future;
use smoltcp::storage::RingBuffer;
use std::io::{self};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Waker};
use std::time::Duration;
use std::{net::SocketAddr, task::Poll};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, BufReader};
use virtual_mio::{ArcInterestHandler, InterestType};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum State {
    Alive,
    Dead,
    Closed,
    Shutdown,
}

#[derive(Debug)]
struct SocketBufferState {
    buffer: RingBuffer<'static, u8>,
    push_handler: Option<ArcInterestHandler>,
    pull_handler: Option<ArcInterestHandler>,
    wakers: Vec<Waker>,
    state: State,
    // This flag prevents a poll write ready storm
    halt_immediate_poll_write: bool,
}

impl SocketBufferState {
    fn add_waker(&mut self, waker: &Waker) {
        if !self.wakers.iter().any(|w| w.will_wake(waker)) {
            self.wakers.push(waker.clone());
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SocketBuffer {
    state: Arc<Mutex<SocketBufferState>>,
    dead_on_drop: bool,
}

impl Drop for SocketBuffer {
    fn drop(&mut self) {
        if self.state() == State::Alive {
            if self.dead_on_drop {
                self.set_state(State::Dead);
            } else {
                self.set_state(State::Closed);
            }
        }
    }
}

impl SocketBuffer {
    fn new(max_size: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(SocketBufferState {
                buffer: RingBuffer::new(vec![0; max_size]),
                push_handler: None,
                pull_handler: None,
                wakers: Vec::new(),
                state: State::Alive,
                halt_immediate_poll_write: false,
            })),
            dead_on_drop: false,
        }
    }

    pub fn set_push_handler(&self, mut handler: ArcInterestHandler) {
        let mut state = self.state.lock().unwrap();
        if state.state != State::Alive {
            handler.push_interest(InterestType::Closed);
        }
        if !state.buffer.is_empty() {
            handler.push_interest(InterestType::Readable);
        }
        state.push_handler.replace(handler);
    }

    pub fn set_pull_handler(&self, mut handler: ArcInterestHandler) {
        let mut state = self.state.lock().unwrap();
        if state.state != State::Alive {
            handler.push_interest(InterestType::Closed);
        }
        if !state.buffer.is_full() && state.pull_handler.is_none() {
            handler.push_interest(InterestType::Writable);
        }
        state.pull_handler.replace(handler);
    }

    pub fn clear_push_handler(&self) {
        let mut state = self.state.lock().unwrap();
        state.push_handler.take();
    }

    pub fn clear_pull_handler(&self) {
        let mut state = self.state.lock().unwrap();
        state.pull_handler.take();
    }

    pub fn poll_read_ready(&self, cx: &mut Context<'_>) -> Poll<crate::Result<usize>> {
        let mut state = self.state.lock().unwrap();
        if !state.buffer.is_empty() {
            return Poll::Ready(Ok(state.buffer.len()));
        }
        match state.state {
            State::Alive => {
                if !state.wakers.iter().any(|w| w.will_wake(cx.waker())) {
                    state.wakers.push(cx.waker().clone());
                }
                Poll::Pending
            }
            State::Dead => {
                tracing::trace!("poll_read_ready: socket is dead");
                Poll::Ready(Err(NetworkError::ConnectionReset))
            }
            State::Closed | State::Shutdown => {
                tracing::trace!("poll_read_ready: socket is closed or shutdown");
                Poll::Ready(Ok(0))
            }
        }
    }

    pub fn poll_write_ready(&self, cx: &mut Context<'_>) -> Poll<crate::Result<usize>> {
        let mut state = self.state.lock().unwrap();
        match state.state {
            State::Alive => {
                if !state.buffer.is_full() && !state.halt_immediate_poll_write {
                    state.halt_immediate_poll_write = true;
                    return Poll::Ready(Ok(state.buffer.window()));
                }
                if !state.wakers.iter().any(|w| w.will_wake(cx.waker())) {
                    state.wakers.push(cx.waker().clone());
                }
                Poll::Pending
            }
            State::Dead => {
                tracing::trace!("poll_write_ready: socket is dead");
                Poll::Ready(Err(NetworkError::ConnectionReset))
            }
            State::Closed | State::Shutdown => {
                tracing::trace!("poll_write_ready: socket is closed or shutdown");
                Poll::Ready(Ok(0))
            }
        }
    }

    fn set_state(&self, new_state: State) {
        let mut state = self.state.lock().unwrap();
        state.state = new_state;
        if let Some(handler) = state.pull_handler.as_mut() {
            handler.push_interest(InterestType::Closed);
        }
        if let Some(handler) = state.push_handler.as_mut() {
            handler.push_interest(InterestType::Closed);
        }
        state.wakers.drain(..).for_each(|w| w.wake());
    }

    fn state(&self) -> State {
        let state = self.state.lock().unwrap();
        state.state
    }

    pub fn try_send(
        &self,
        data: &[u8],
        all_or_nothing: bool,
        waker: Option<&Waker>,
    ) -> crate::Result<usize> {
        let mut state = self.state.lock().unwrap();
        if state.state != State::Alive {
            return Err(NetworkError::ConnectionReset);
        }
        state.halt_immediate_poll_write = false;
        let available = state.buffer.window();
        if available == 0 {
            if let Some(waker) = waker {
                state.add_waker(waker)
            }
            return Err(NetworkError::WouldBlock);
        }
        if data.len() > available {
            if all_or_nothing {
                if let Some(waker) = waker {
                    state.add_waker(waker)
                }
                return Err(NetworkError::WouldBlock);
            }
            let amt = state.buffer.enqueue_slice(&data[..available]);
            return Ok(amt);
        }
        let amt = state.buffer.enqueue_slice(data);

        if let Some(handler) = state.push_handler.as_mut() {
            handler.push_interest(InterestType::Readable);
        }
        state.wakers.drain(..).for_each(|w| w.wake());
        Ok(amt)
    }

    pub async fn send(&self, data: Bytes) -> crate::Result<()> {
        struct Poller<'a> {
            this: &'a SocketBuffer,
            data: Bytes,
        }
        impl<'a> Future for Poller<'a> {
            type Output = crate::Result<()>;
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                loop {
                    if self.data.is_empty() {
                        return Poll::Ready(Ok(()));
                    }
                    return match self.this.try_send(&self.data, false, Some(cx.waker())) {
                        Ok(amt) => {
                            self.data.advance(amt);
                            continue;
                        }
                        Err(NetworkError::WouldBlock) => Poll::Pending,
                        Err(err) => Poll::Ready(Err(err)),
                    };
                }
            }
        }
        Poller { this: self, data }.await
    }

    pub fn try_read(
        &self,
        buf: &mut [std::mem::MaybeUninit<u8>],
        waker: Option<&Waker>,
    ) -> crate::Result<usize> {
        let mut state = self.state.lock().unwrap();
        if state.buffer.is_empty() {
            return match state.state {
                State::Alive => {
                    if let Some(waker) = waker {
                        state.add_waker(waker)
                    }
                    Err(NetworkError::WouldBlock)
                }
                State::Dead => {
                    tracing::trace!("try_read: socket is dead");
                    return Err(NetworkError::ConnectionReset);
                }
                State::Closed | State::Shutdown => {
                    tracing::trace!("try_read: socket is closed or shutdown");
                    return Ok(0);
                }
            };
        }

        let buf: &mut [u8] = unsafe { std::mem::transmute(buf) };
        let amt = buf.len().min(state.buffer.len());
        let amt = state.buffer.dequeue_slice(&mut buf[..amt]);

        if let Some(handler) = state.pull_handler.as_mut() {
            handler.push_interest(InterestType::Writable);
        }
        state.wakers.drain(..).for_each(|w| w.wake());
        Ok(amt)
    }

    pub fn set_max_size(&self, new_size: usize) {
        let mut state = self.state.lock().unwrap();
        state.halt_immediate_poll_write = false;

        let mut existing: Vec<u8> = vec![0; state.buffer.len()];
        if !state.buffer.is_empty() {
            let amt = state.buffer.dequeue_slice(&mut existing[..]);
            existing.resize(amt, 0);
        }

        state.buffer = RingBuffer::new(vec![0; new_size]);
        if !existing.is_empty() {
            let _ = state.buffer.enqueue_slice(&existing[..]);
        }
    }

    pub fn max_size(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.buffer.capacity()
    }
}

impl AsyncWrite for SocketBuffer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.try_send(buf, false, Some(cx.waker())) {
            Ok(amt) => Poll::Ready(Ok(amt)),
            Err(NetworkError::WouldBlock) => Poll::Pending,
            Err(err) => Poll::Ready(Err(net_error_into_io_err(err))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.set_state(State::Shutdown);
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for SocketBuffer {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.try_read(unsafe { buf.unfilled_mut() }, Some(cx.waker())) {
            Ok(amt) => {
                unsafe { buf.assume_init(amt) };
                buf.advance(amt);
                Poll::Ready(Ok(()))
            }
            Err(NetworkError::WouldBlock) => Poll::Pending,
            Err(err) => Poll::Ready(Err(net_error_into_io_err(err))),
        }
    }
}

#[derive(Debug)]
pub struct TcpSocketHalf {
    addr_local: SocketAddr,
    addr_peer: SocketAddr,
    tx: SocketBuffer,
    rx: SocketBuffer,
    ttl: u32,
}

impl TcpSocketHalf {
    pub fn channel(
        max_buffer_size: usize,
        addr1: SocketAddr,
        addr2: SocketAddr,
    ) -> (TcpSocketHalf, TcpSocketHalf) {
        let mut buffer1 = SocketBuffer::new(max_buffer_size);
        buffer1.dead_on_drop = true;

        let mut buffer2 = SocketBuffer::new(max_buffer_size);
        buffer2.dead_on_drop = true;

        let half1 = Self {
            tx: buffer1.clone(),
            rx: buffer2.clone(),
            addr_local: addr1,
            addr_peer: addr2,
            ttl: 64,
        };
        let half2 = Self {
            tx: buffer2,
            rx: buffer1,
            addr_local: addr2,
            addr_peer: addr1,
            ttl: 64,
        };
        (half1, half2)
    }

    pub fn is_active(&self) -> bool {
        self.tx.state() == State::Alive
    }

    pub fn close(&self) -> crate::Result<()> {
        self.tx.set_state(State::Closed);
        self.rx.set_state(State::Closed);
        Ok(())
    }
}

impl VirtualIoSource for TcpSocketHalf {
    fn remove_handler(&mut self) {
        self.tx.clear_pull_handler();
        self.rx.clear_push_handler();
    }

    fn poll_read_ready(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<usize>> {
        self.rx.poll_read_ready(cx)
    }

    fn poll_write_ready(&mut self, cx: &mut Context<'_>) -> Poll<crate::Result<usize>> {
        self.tx.poll_write_ready(cx)
    }
}

impl VirtualSocket for TcpSocketHalf {
    fn set_ttl(&mut self, ttl: u32) -> crate::Result<()> {
        self.ttl = ttl;
        Ok(())
    }

    fn ttl(&self) -> crate::Result<u32> {
        Ok(self.ttl)
    }

    fn addr_local(&self) -> crate::Result<SocketAddr> {
        Ok(self.addr_local)
    }

    fn status(&self) -> crate::Result<SocketStatus> {
        Ok(match self.tx.state() {
            State::Alive => SocketStatus::Opened,
            State::Dead => SocketStatus::Failed,
            State::Closed => SocketStatus::Closed,
            State::Shutdown => SocketStatus::Closed,
        })
    }

    fn set_handler(
        &mut self,
        handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> crate::Result<()> {
        let handler = ArcInterestHandler::new(handler);
        self.tx.set_pull_handler(handler.clone());
        self.rx.set_push_handler(handler);
        Ok(())
    }
}

impl VirtualConnectedSocket for TcpSocketHalf {
    fn set_linger(&mut self, _linger: Option<Duration>) -> crate::Result<()> {
        Ok(())
    }

    fn linger(&self) -> crate::Result<Option<Duration>> {
        Ok(None)
    }

    fn try_send(&mut self, data: &[u8]) -> crate::Result<usize> {
        self.tx.try_send(data, false, None)
    }

    fn try_flush(&mut self) -> crate::Result<()> {
        Ok(())
    }

    fn close(&mut self) -> crate::Result<()> {
        self.tx.set_state(State::Closed);
        self.rx.set_state(State::Closed);
        Ok(())
    }

    fn try_recv(&mut self, buf: &mut [std::mem::MaybeUninit<u8>]) -> crate::Result<usize> {
        self.rx.try_read(buf, None)
    }
}

impl VirtualTcpSocket for TcpSocketHalf {
    fn set_recv_buf_size(&mut self, size: usize) -> crate::Result<()> {
        self.rx.set_max_size(size);
        Ok(())
    }

    fn recv_buf_size(&self) -> crate::Result<usize> {
        Ok(self.rx.max_size())
    }

    fn set_send_buf_size(&mut self, size: usize) -> crate::Result<()> {
        self.tx.set_max_size(size);
        Ok(())
    }

    fn send_buf_size(&self) -> crate::Result<usize> {
        Ok(self.tx.max_size())
    }

    fn set_nodelay(&mut self, _reuse: bool) -> crate::Result<()> {
        Ok(())
    }

    fn nodelay(&self) -> crate::Result<bool> {
        Ok(true)
    }

    fn set_keepalive(&mut self, _keepalive: bool) -> crate::Result<()> {
        Ok(())
    }

    fn keepalive(&self) -> crate::Result<bool> {
        Ok(false)
    }

    fn set_dontroute(&mut self, _keepalive: bool) -> crate::Result<()> {
        Ok(())
    }

    fn dontroute(&self) -> crate::Result<bool> {
        Ok(false)
    }

    fn addr_peer(&self) -> crate::Result<SocketAddr> {
        Ok(self.addr_peer)
    }

    fn shutdown(&mut self, how: std::net::Shutdown) -> crate::Result<()> {
        match how {
            std::net::Shutdown::Both => {
                self.tx.set_state(State::Shutdown);
                self.rx.set_state(State::Shutdown);
            }
            std::net::Shutdown::Read => {
                self.rx.set_state(State::Shutdown);
            }
            std::net::Shutdown::Write => {
                self.tx.set_state(State::Shutdown);
            }
        }
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.tx.state() != State::Alive
    }
}

#[allow(unused)]
#[derive(Debug)]
pub struct TcpSocketHalfTx {
    addr_local: SocketAddr,
    addr_peer: SocketAddr,
    tx: SocketBuffer,
    ttl: u32,
}

impl TcpSocketHalfTx {
    pub fn poll_send(&self, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        match self.tx.try_send(data, false, Some(cx.waker())) {
            Ok(amt) => Poll::Ready(Ok(amt)),
            Err(NetworkError::WouldBlock) => Poll::Pending,
            Err(err) => Poll::Ready(Err(net_error_into_io_err(err))),
        }
    }

    pub fn try_send(&self, data: &[u8]) -> io::Result<usize> {
        self.tx
            .try_send(data, false, None)
            .map_err(net_error_into_io_err)
    }

    pub async fn send_ext(&self, data: Bytes, non_blocking: bool) -> io::Result<()> {
        if non_blocking {
            self.tx
                .try_send(&data, true, None)
                .map_err(net_error_into_io_err)
                .map(|_| ())
        } else {
            self.tx.send(data).await.map_err(net_error_into_io_err)
        }
    }

    pub async fn send(&self, data: Bytes) -> io::Result<()> {
        self.send_ext(data, false).await
    }

    pub fn close(&self) -> crate::Result<()> {
        self.tx.set_state(State::Closed);
        Ok(())
    }
}

impl AsyncWrite for TcpSocketHalfTx {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.tx).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.tx).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.tx).poll_shutdown(cx)
    }
}

#[allow(unused)]
#[derive(Debug)]
pub struct TcpSocketHalfRx {
    addr_local: SocketAddr,
    addr_peer: SocketAddr,
    rx: BufReader<SocketBuffer>,
    ttl: u32,
}

impl TcpSocketHalfRx {
    pub fn buffer(&self) -> &[u8] {
        self.rx.buffer()
    }

    pub fn close(&mut self) -> crate::Result<()> {
        self.rx.get_mut().set_state(State::Closed);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn inner(&mut self) -> &BufReader<SocketBuffer> {
        &self.rx
    }

    #[allow(dead_code)]
    pub(crate) fn inner_mut(&mut self) -> &mut BufReader<SocketBuffer> {
        &mut self.rx
    }
}

impl AsyncRead for TcpSocketHalfRx {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.rx).poll_read(cx, buf)
    }
}

impl TcpSocketHalfRx {
    pub fn poll_fill_buf(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        Pin::new(&mut self.rx).poll_fill_buf(cx)
    }

    pub fn consume(&mut self, amt: usize) {
        Pin::new(&mut self.rx).consume(amt)
    }
}

impl TcpSocketHalf {
    pub fn split(self) -> (TcpSocketHalfTx, TcpSocketHalfRx) {
        let tx = TcpSocketHalfTx {
            tx: self.tx,
            addr_local: self.addr_local,
            addr_peer: self.addr_peer,
            ttl: self.ttl,
        };
        let rx = TcpSocketHalfRx {
            rx: BufReader::new(self.rx),
            addr_local: self.addr_local,
            addr_peer: self.addr_peer,
            ttl: self.ttl,
        };
        (tx, rx)
    }

    pub fn combine(tx: TcpSocketHalfTx, rx: TcpSocketHalfRx) -> Self {
        Self {
            tx: tx.tx,
            rx: rx.rx.into_inner(),
            addr_local: tx.addr_local,
            addr_peer: tx.addr_peer,
            ttl: tx.ttl,
        }
    }
}
