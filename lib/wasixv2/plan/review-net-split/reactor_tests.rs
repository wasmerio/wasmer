#[cfg(test)]
mod tests {
    use std::{os::fd::FromRawFd, sync::mpsc};

    use super::*;

    fn backend() -> Arc<HostBackend> {
        HostBackend::new().unwrap()
    }

    #[test]
    fn keepalive_parameters_reach_the_host_socket_once_keepalive_is_on() {
        let b = backend();
        let host = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let sock = TcpStream::connect(host.local_addr().unwrap()).unwrap();
        let peer = sock.peer_addr().unwrap_or(host.local_addr().unwrap());
        let s = HostStream::register(&b.shared, sock, peer, false, WaitQueue::new()).unwrap();
        let int = |v: i32| v.to_le_bytes();
        let tcp = SocketLevel::SOL_TCP.0;
        s.set_option(tcp, TcpOption::TCP_KEEPIDLE.0, &int(42))
            .unwrap();
        let host_sock = socket2::SockRef::from(&s.0.sock);
        // Stored, not applied, while keepalive is off.
        assert!(!host_sock.keepalive().unwrap());
        s.set_option(
            SocketLevel::SOL_SOCKET.0,
            SocketOption::SO_KEEPALIVE.0,
            &int(1),
        )
        .unwrap();
        assert!(host_sock.keepalive().unwrap());
        assert_eq!(
            host_sock.tcp_keepalive_time().unwrap(),
            Duration::from_secs(42)
        );
        s.set_option(tcp, TcpOption::TCP_NODELAY.0, &int(1))
            .unwrap();
        assert!(host_sock.tcp_nodelay().unwrap());
        s.set_option(
            SocketLevel::SOL_SOCKET.0,
            SocketOption::SO_KEEPALIVE.0,
            &int(0),
        )
        .unwrap();
        assert!(!host_sock.keepalive().unwrap());
    }

    #[test]
    fn a_datagram_endpoint_is_writable_until_the_host_is_full() {
        let b = backend();
        let wq = WaitQueue::new();
        let d = b.datagram(Domain::Inet, wq.clone()).unwrap();
        assert!(d.poll(None).contains(PollMask::OUT));
        let (tx, rx) = mpsc::channel();
        struct Signal(mpsc::Sender<PollMask>);
        impl WaitEntry for Signal {
            fn wake(&self, events: PollMask) -> bool {
                let _ = self.0.send(events);
                true
            }
        }
        let _reg = wq.add(Arc::new(Signal(tx)));
        // What a send meeting `EAGAIN` leaves.
        let shared = b
            .shared
            .entries
            .lock()
            .unwrap()
            .values()
            .find_map(|e| match e {
                Entry::Datagram(d) => Some(d.clone()),
                _ => None,
            });
        let shared = shared.unwrap();
        shared.state().host_writable = false;
        assert!(!d.poll(None).intersects(PollMask::OUT | PollMask::WRNORM));
        // The reactor's writable event wakes the guest socket's queue with OUT.
        shared.writable();
        assert!(rx.try_recv().unwrap().contains(PollMask::OUT));
        assert!(d.poll(None).contains(PollMask::OUT));
    }

    #[test]
    fn a_persistent_receive_error_does_not_spin_the_reactor() {
        let mut fds = [0; 2];
        // SAFETY: `pipe` fills both descriptors.
        assert_eq!(unsafe { libc_pipe(fds.as_mut_ptr()) }, 0);
        // A descriptor that is not a socket fails every receive with ENOTSOCK.
        // SAFETY: the read end is ours and closed when the socket drops.
        let sock = unsafe { std::net::UdpSocket::from_raw_fd(fds[0]) };
        let shared = DgramShared {
            sock: UdpSocket::from_std(sock),
            token: Token(1),
            wq: WaitQueue::new(),
            st: Mutex::new(DgramState {
                queue: VecDeque::new(),
                queued: 0,
                err: None,
                connected: Some("127.0.0.1:9".parse().unwrap()),
                host_writable: true,
            }),
            backend: Weak::new(),
        };
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            shared.receive_all(&mut [0u8; 64]);
            let _ = tx.send(shared.state().err);
        });
        let err = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("receive_all returns");
        assert_eq!(err, Some(Errno::ENOTSOCK));
        // SAFETY: the write end is still open and ours.
        unsafe { libc_close(fds[1]) };
    }

    unsafe extern "C" {
        #[link_name = "pipe"]
        fn libc_pipe(fds: *mut i32) -> i32;
        #[link_name = "close"]
        fn libc_close(fd: i32) -> i32;
    }
}
