use super::*;
use futures::executor::block_on;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};
use wasixv2_host::{ByteSink, ByteSource};

#[derive(Default)]
struct Count(AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
fn poll<T>(future: Pin<&mut (impl Future<Output = T> + ?Sized)>) -> Poll<T> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}
fn kind<T: std::fmt::Debug>(result: io::Result<T>, expected: io::ErrorKind) {
    assert_eq!(result.unwrap_err().kind(), expected);
}

#[test]
fn bounded_partial_writes_and_persistent_eof() {
    kind(channel(0).map(|_| ()), io::ErrorKind::InvalidInput);
    let (tx, rx) = channel(3).unwrap();
    assert_eq!(tx.try_write(b"abcd").unwrap(), 3);
    kind(tx.try_write(b"d"), io::ErrorKind::WouldBlock);
    let mut bytes = [0; 4];
    assert_eq!(rx.try_read(&mut bytes[..2]).unwrap(), 2);
    assert_eq!(&bytes[..2], b"ab");
    assert_eq!(tx.try_write(b"de").unwrap(), 2);
    tx.close();
    kind(tx.try_write(b"f"), io::ErrorKind::BrokenPipe);
    assert_eq!(rx.try_read(&mut bytes).unwrap(), 3);
    assert_eq!(&bytes[..3], b"cde");
    for _ in 0..2 {
        assert_eq!(rx.try_read(&mut bytes).unwrap(), 0);
    }
}

#[test]
fn empty_buffers_do_not_consume_data_or_require_readiness() {
    let (tx, rx) = channel(1).unwrap();
    assert_eq!(rx.try_read(&mut []).unwrap(), 0);
    assert_eq!(tx.try_write(b"").unwrap(), 0);
    tx.try_write(b"a").unwrap();
    assert_eq!(tx.try_write(b"").unwrap(), 0);
    assert_eq!(rx.try_read(&mut []).unwrap(), 0);
    assert_eq!(rx.try_read(&mut [0]).unwrap(), 1);
}

#[test]
fn producer_drop_delivers_remaining_bytes_then_eof() {
    let (tx, rx) = channel(2).unwrap();
    tx.try_write(b"ab").unwrap();
    drop(tx);
    let mut bytes = [0; 2];
    assert_eq!(block_on(rx.read(&mut bytes)).unwrap(), 2);
    assert_eq!(&bytes, b"ab");
    assert_eq!(block_on(rx.read(&mut bytes)).unwrap(), 0);
}

#[test]
fn every_blocked_writer_is_woken_and_cancellation_keeps_bytes() {
    let (tx, rx) = channel(1).unwrap();
    tx.try_write(b"a").unwrap();
    let mut first = tx.write(b"b");
    let mut second = tx.write(b"c");
    let flag = Arc::new(Count::default());
    let waker = Waker::from(flag.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(first.as_mut().poll(&mut cx).is_pending());
    assert!(second.as_mut().poll(&mut cx).is_pending());
    drop(first);
    let mut bytes = [0];
    rx.try_read(&mut bytes).unwrap();
    assert_eq!(bytes, *b"a");
    assert!(flag.0.load(Ordering::Relaxed) >= 1);
    assert!(matches!(second.as_mut().poll(&mut cx), Poll::Ready(Ok(1))));
    rx.try_read(&mut bytes).unwrap();
    assert_eq!(bytes, *b"c");
}

#[test]
fn read_and_drain_wakes_cross_threads_without_lost_wakes() {
    let (tx, rx) = channel(7).unwrap();
    let writer = std::thread::spawn(move || {
        for i in 0..2000u32 {
            let data = i.to_le_bytes();
            let mut left = &data[..];
            while !left.is_empty() {
                let n = block_on(tx.write(left)).unwrap();
                left = &left[n..];
            }
        }
        tx.close();
        block_on(tx.drained()).unwrap();
    });
    let mut result = Vec::new();
    let mut bytes = [0; 5];
    loop {
        let n = block_on(rx.read(&mut bytes)).unwrap();
        if n == 0 {
            break;
        }
        result.extend_from_slice(&bytes[..n]);
    }
    writer.join().unwrap();
    let expected: Vec<_> = (0..2000u32).flat_map(u32::to_le_bytes).collect();
    assert_eq!(result, expected);
}

#[test]
fn source_poll_wakes_on_input_eof_and_abort() {
    for abort in [false, true] {
        let (tx, rx) = channel(1).unwrap();
        let count = Arc::new(Count::default());
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        let mut bytes = [0];
        assert!(rx.poll_read(&mut cx, &mut bytes).is_pending());
        tx.try_write(b"a").unwrap();
        assert!(count.0.load(Ordering::Relaxed) > 0);
        assert!(matches!(
            rx.poll_read(&mut cx, &mut bytes),
            Poll::Ready(Ok(1))
        ));
        assert!(rx.poll_read(&mut cx, &mut bytes).is_pending());
        if abort {
            tx.abort();
        } else {
            tx.close();
        }
        assert!(count.0.load(Ordering::Relaxed) >= 2);
        let result = rx.poll_read(&mut cx, &mut bytes);
        if abort {
            assert!(
                matches!(result, Poll::Ready(Err(e)) if e.kind() == io::ErrorKind::ConnectionAborted)
            );
        } else {
            assert!(matches!(result, Poll::Ready(Ok(0))));
        }
    }
}

#[test]
fn consumer_cancel_wakes_writer_and_drain_and_discards_buffer() {
    let (tx, rx) = channel(1).unwrap();
    tx.try_write(b"a").unwrap();
    let mut write = tx.write(b"b");
    let mut drained = Box::pin(tx.drained());
    assert!(poll(write.as_mut()).is_pending());
    assert!(poll(drained.as_mut()).is_pending());
    rx.cancel();
    kind(block_on(write), io::ErrorKind::ConnectionAborted);
    kind(block_on(drained), io::ErrorKind::ConnectionAborted);
    kind(rx.try_read(&mut [0]), io::ErrorKind::ConnectionAborted);
    rx.cancel();
}

#[test]
fn dropping_consumer_and_closing_producer_release_pending_writes() {
    let (tx, rx) = channel(1).unwrap();
    tx.try_write(b"a").unwrap();
    let mut write = tx.write(b"b");
    assert!(poll(write.as_mut()).is_pending());
    drop(rx);
    kind(block_on(write), io::ErrorKind::ConnectionAborted);

    let (tx, _rx) = channel(1).unwrap();
    tx.try_write(b"a").unwrap();
    let mut write = tx.write(b"b");
    assert!(poll(write.as_mut()).is_pending());
    tx.close();
    kind(block_on(write), io::ErrorKind::BrokenPipe);
}

#[test]
fn dropped_read_registration_cannot_steal_a_later_wake() {
    let (tx, rx) = channel(1).unwrap();
    let mut bytes = [0];
    let mut read = Box::pin(rx.read(&mut bytes));
    assert!(poll(read.as_mut()).is_pending());
    drop(read);
    tx.try_write(b"x").unwrap();
    assert_eq!(block_on(rx.read(&mut bytes)).unwrap(), 1);
    assert_eq!(bytes, *b"x");
}

#[test]
fn coordinator_wakes_after_payload_unlock() {
    use std::sync::Weak;
    struct Coordinator {
        source: Weak<Source>,
        calls: AtomicUsize,
    }
    impl StreamWake for Coordinator {
        fn notify(&self) {
            if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
                // Consume actual payload, then reenter wake and set_wake. An empty read
                // would bypass the state lock and would not test unlock ordering.
                let source = self.source.upgrade().unwrap();
                let mut bytes = [0];
                assert_eq!(source.try_read(&mut bytes).unwrap(), 1);
                assert_eq!(bytes, *b"x");
                source.set_wake(Arc::new(Waker::noop().clone()));
            }
        }
    }
    let (tx, rx) = channel(1).unwrap();
    let rx = Arc::new(rx);
    let wake = Arc::new(Coordinator {
        source: Arc::downgrade(&rx),
        calls: AtomicUsize::new(0),
    });
    rx.set_wake(wake.clone());
    tx.try_write(b"x").unwrap();
    assert_eq!(wake.calls.load(Ordering::Relaxed), 2);
    kind(rx.try_read(&mut [0]), io::ErrorKind::WouldBlock);
}

#[test]
fn finish_waits_for_both_outputs_with_independent_backpressure() {
    let (session, stdio, streams) = Session::new(2).unwrap();
    assert_eq!(block_on(stdio.stdout.write(b"ab")).unwrap(), 2);
    let mut blocked = stdio.stdout.write(b"c");
    assert!(poll(blocked.as_mut()).is_pending());
    assert_eq!(block_on(stdio.stderr.write(b"!")).unwrap(), 1);
    drop(blocked);
    let mut finish = Box::pin(session.finish());
    assert!(poll(finish.as_mut()).is_pending());
    let mut bytes = [0; 2];
    assert_eq!(streams.stdout.try_read(&mut bytes).unwrap(), 2);
    assert_eq!(&bytes, b"ab");
    assert!(poll(finish.as_mut()).is_pending());
    assert_eq!(streams.stderr.try_read(&mut bytes).unwrap(), 1);
    assert_eq!(bytes[0], b'!');
    assert!(matches!(poll(finish.as_mut()), Poll::Ready(Ok(()))));
    assert_eq!(streams.stdout.try_read(&mut bytes).unwrap(), 0);
    assert_eq!(streams.stderr.try_read(&mut bytes).unwrap(), 0);
    kind(
        streams.stdin.try_write(b"x"),
        io::ErrorKind::ConnectionAborted,
    );
}

#[test]
fn abort_releases_every_stream_and_finish_reports_failure() {
    let (session, stdio, streams) = Session::new(1).unwrap();
    block_on(stdio.stdout.write(b"a")).unwrap();
    block_on(stdio.stderr.write(b"b")).unwrap();
    session.abort();
    kind(block_on(session.finish()), io::ErrorKind::ConnectionAborted);
    kind(
        streams.stdout.try_read(&mut [0]),
        io::ErrorKind::ConnectionAborted,
    );
    kind(
        streams.stderr.try_read(&mut [0]),
        io::ErrorKind::ConnectionAborted,
    );
    kind(
        streams.stdin.try_write(b"x"),
        io::ErrorKind::ConnectionAborted,
    );
}

#[test]
fn console_preserves_raw_bytes_and_keeps_stderr_separate() {
    let (session, stdio, streams) = Session::new(8).unwrap();
    let size = WindowSize {
        rows: 24,
        cols: 80,
        ..WindowSize::default()
    };
    let console = session.console(size);
    assert_eq!(console.size, size);
    assert!(Arc::ptr_eq(&console.input, &stdio.stdin));
    assert!(Arc::ptr_eq(&console.output, &stdio.stdout));
    let raw = b"a\x7f\x03\x04\r";
    streams.stdin.try_write(raw).unwrap();
    let mut bytes = [0; 5];
    assert!(matches!(
        console
            .input
            .poll_read(&mut Context::from_waker(Waker::noop()), &mut bytes),
        Poll::Ready(Ok(5))
    ));
    assert_eq!(&bytes, raw);
    block_on(stdio.stderr.write(b"err")).unwrap();
    kind(
        streams.stdout.try_read(&mut bytes),
        io::ErrorKind::WouldBlock,
    );
    assert_eq!(streams.stderr.try_read(&mut bytes).unwrap(), 3);
}
