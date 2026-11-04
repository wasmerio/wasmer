//! Actual v1 reference relay over bounded framed channels, with real loopback sockets.
//! Websocket ownership/framing belongs to the worker-local transport and is not exercised here.
#![cfg(unix)]
use std::{
    future::Future,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, UdpSocket},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::mpsc;
use virtual_net::{
    RemoteNetworkingServer, RemoteNetworkingServerDriver,
    host::LocalNetworking,
    meta::{MessageRequest, MessageResponse},
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{Answer, Backend, Domain, Lookup, Shutdown, StreamState};
use wasixv2_vnet_web::{
    Capabilities, Classic, Config, Driver, FrameTransport, Transport, WebBackend,
};
use wasixv2_wait::WaitQueue;

struct ReferenceRelay {
    tx: Option<mpsc::Sender<MessageRequest>>,
    rx: mpsc::Receiver<MessageResponse>,
    driver: Pin<Box<RemoteNetworkingServerDriver>>,
    _reference: RemoteNetworkingServer,
}
impl ReferenceRelay {
    fn new() -> Self {
        let (tx, requests) = mpsc::channel(8);
        let (responses, rx) = mpsc::channel(8);
        let (reference, driver) = RemoteNetworkingServer::new_from_mpsc(
            responses,
            requests,
            Arc::new(LocalNetworking::new()),
        );
        Self {
            tx: Some(tx),
            rx,
            driver: Box::pin(driver),
            _reference: reference,
        }
    }
}
impl FrameTransport for ReferenceRelay {
    fn poll_send(&mut self, cx: &mut Context<'_>, frame: &[u8]) -> Poll<Result<(), Errno>> {
        if self.driver.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(Errno::ECONNABORTED));
        }
        let (message, used) =
            bincode::serde::decode_from_slice(frame, bincode::config::legacy()).unwrap();
        assert_eq!(used, frame.len());
        match self.tx.as_ref().unwrap().try_send(message) {
            Ok(()) => {
                cx.waker().wake_by_ref();
                Poll::Ready(Ok(()))
            }
            Err(mpsc::error::TrySendError::Full(_)) => Poll::Pending,
            Err(mpsc::error::TrySendError::Closed(_)) => Poll::Ready(Err(Errno::ECONNABORTED)),
        }
    }
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<Vec<u8>, Errno>> {
        let _ = self.driver.as_mut().poll(cx);
        self.rx.poll_recv(cx).map(|message| {
            message
                .map(|message| {
                    bincode::serde::encode_to_vec(message, bincode::config::legacy()).unwrap()
                })
                .ok_or(Errno::ECONNABORTED)
        })
    }
    fn poll_closed(&mut self, cx: &mut Context<'_>) -> Poll<Errno> {
        if self.driver.as_mut().poll(cx).is_ready() {
            Poll::Ready(Errno::ECONNABORTED)
        } else {
            Poll::Pending
        }
    }
    fn close(&mut self) {
        self.tx.take();
        self.rx.close();
    }
}
async fn drive_until(
    stage: &str,
    driver: &mut Driver,
    relay: &mut dyn Transport,
    mut ready: impl FnMut() -> bool,
) {
    tokio::time::timeout(
        Duration::from_secs(5),
        futures_util::future::poll_fn(|cx| {
            assert!(driver.poll(cx, relay).is_pending(), "relay driver failed");
            if ready() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    )
    .await
    .unwrap_or_else(|_| panic!("local relay timed out: {stage}"));
}

fn exercise(ip: IpAddr) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let capabilities = Capabilities::classic(ip.is_ipv6());
        let (backend, mut driver) = WebBackend::new(Config::default(), capabilities).unwrap();
        let mut relay = Classic::new(ReferenceRelay::new(), capabilities);
        let listener = TcpListener::bind(SocketAddr::new(ip, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let (finished, finish) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut data = [0; 12];
            peer.read_exact(&mut data).unwrap();
            peer.write_all(&data).unwrap();
            let mut rest = Vec::new();
            peer.read_to_end(&mut rest).unwrap();
            assert!(rest.is_empty());
            finished.send(()).unwrap();
        });
        let stream = backend.connect(address, WaitQueue::new()).unwrap();
        drive_until("connect", &mut driver, &mut relay, || stream.state() != StreamState::Connecting).await;
        assert_eq!(stream.state(), StreamState::Established);
        assert_eq!(stream.take_error(), None);
        assert_eq!(stream.write(b"hello relay!"), Ok(12));
        let mut received = Vec::new();
        drive_until("echo", &mut driver, &mut relay, || {
            let mut data = [0; 16];
            if let Ok(n) = stream.read(&mut data, false) { received.extend_from_slice(&data[..n]); }
            received.len() == 12
        }).await;
        assert_eq!(received, b"hello relay!");
        stream.shutdown(Shutdown::Write).unwrap();
        // Stock relay omits EOF events. Assert local FIN reaches the peer without claiming
        // the stock server can report a remote half-close (model tests cover that event).
        drive_until("shutdown", &mut driver, &mut relay, || finish.try_recv().is_ok()).await;
        server.join().unwrap();

        let udp = UdpSocket::bind(SocketAddr::new(ip, 0)).unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let address = udp.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut data = [0; 16];
            let (n, sender) = udp.recv_from(&mut data).unwrap();
            udp.send_to(&data[..n], sender).unwrap();
        });
        let domain = if ip.is_ipv4() { Domain::Inet } else { Domain::Inet6 };
        let datagram = backend.datagram(domain, WaitQueue::new()).unwrap();
        let mut submitted = false;
        let mut reply = None;
        drive_until("udp", &mut driver, &mut relay, || {
            if !submitted {
                match datagram.send_to(b"packet", address) {
                    Ok(6) => submitted = true,
                    Err(Errno::EAGAIN) => {},
                    other => panic!("unexpected send: {other:?}"),
                }
            }
            let mut data = [0; 3];
            if let Ok((n, full, from)) = datagram.recv_from(&mut data, false) { reply = Some((data, n, full, from)); }
            reply.is_some()
        }).await;
        assert_eq!(reply, Some((*b"pac", 3, 6, address)));
        server.join().unwrap();

        let found = Arc::new(Mutex::new(None));
        let output = found.clone();
        backend.lookup(Lookup::Name("localhost".into()), Box::new(move |result| *output.lock().unwrap() = Some(result)));
        drive_until("dns", &mut driver, &mut relay, || found.lock().unwrap().is_some()).await;
        let found = found.lock().unwrap().take().unwrap().unwrap();
        assert!(matches!(found.answer, Answer::Addrs { addrs, .. } if addrs.iter().any(|ip| ip.is_loopback())));
    });
}
#[test]
fn reference_relay_tcp_udp_dns_ipv4() {
    exercise("127.0.0.1".parse().unwrap());
}
#[test]
fn reference_relay_tcp_udp_dns_ipv6() {
    exercise("::1".parse().unwrap());
}

#[derive(Default)]
struct Ingress(Mutex<Vec<Arc<dyn wasixv2_vnet::BackendStream>>>);
impl wasixv2_vnet::InboundSink for Ingress {
    fn poll_room(&self, _: Option<&mut wasixv2_wait::PollTable<'_>>) -> wasixv2_vnet::Room {
        wasixv2_vnet::Room::Ready
    }
    fn incoming(&self, stream: Arc<dyn wasixv2_vnet::BackendStream>, _: WaitQueue) -> bool {
        self.0.lock().unwrap().push(stream);
        true
    }
}
fn ephemeral_publications(ip: IpAddr) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (backend, mut driver) =
                WebBackend::new(Config::default(), Capabilities::classic_fidelity(true)).unwrap();
            let mut relay =
                Classic::new(ReferenceRelay::new(), Capabilities::classic_fidelity(true));
            let sink = Arc::new(Ingress::default());
            let mut bind =
                Box::pin(backend.publish_resolved(SocketAddr::new(ip, 0), 4, sink.clone()));
            let publication = tokio::time::timeout(
                Duration::from_secs(5),
                futures_util::future::poll_fn(|cx| {
                    assert!(driver.poll(cx, &mut relay).is_pending());
                    bind.as_mut().poll(cx)
                }),
            )
            .await
            .unwrap()
            .unwrap();
            drop(bind);
            let host = publication.host_addr();
            assert_eq!(host.ip(), ip);
            assert_ne!(host.port(), 0);
            let mut client = std::net::TcpStream::connect(host).unwrap();
            client.write_all(b"ingress").unwrap();
            drive_until("ephemeral TCP ingress", &mut driver, &mut relay, || {
                !sink.0.lock().unwrap().is_empty()
            })
            .await;
            let stream = sink.0.lock().unwrap().pop().unwrap();
            let mut bytes = Vec::new();
            drive_until("ephemeral TCP bytes", &mut driver, &mut relay, || {
                let mut buf = [0; 16];
                if let Ok(n) = stream.read(&mut buf, false) {
                    bytes.extend_from_slice(&buf[..n]);
                }
                bytes.len() == 7
            })
            .await;
            assert_eq!(bytes, b"ingress");
            drop(stream);
            drop(client);
            drop(publication);

            let mut bind = Box::pin(backend.publish_datagram_resolved(SocketAddr::new(ip, 0)));
            let (endpoint, publication) = tokio::time::timeout(
                Duration::from_secs(5),
                futures_util::future::poll_fn(|cx| {
                    assert!(driver.poll(cx, &mut relay).is_pending());
                    bind.as_mut().poll(cx)
                }),
            )
            .await
            .unwrap()
            .unwrap();
            drop(bind);
            let host = publication.host_addr();
            assert_eq!(host.ip(), ip);
            assert_ne!(host.port(), 0);
            endpoint.attach(Some(WaitQueue::new())).unwrap();
            let client = UdpSocket::bind(SocketAddr::new(ip, 0)).unwrap();
            client.set_nonblocking(true).unwrap();
            client.send_to(b"udp", host).unwrap();
            let mut received = None;
            drive_until("ephemeral UDP ingress", &mut driver, &mut relay, || {
                let mut buf = [0; 8];
                if let Ok((n, _, peer)) = endpoint.recv_from(&mut buf, false) {
                    received = Some((buf[..n].to_vec(), peer));
                }
                received.is_some()
            })
            .await;
            let (bytes, peer) = received.unwrap();
            assert_eq!(bytes, b"udp");
            assert_eq!(endpoint.send_to(b"reply", peer), Ok(5));
            let mut received = None;
            drive_until(
                "ephemeral UDP reply source",
                &mut driver,
                &mut relay,
                || {
                    let mut buf = [0; 8];
                    if let Ok((n, peer)) = client.recv_from(&mut buf) {
                        received = Some((buf[..n].to_vec(), peer));
                    }
                    received.is_some()
                },
            )
            .await;
            assert_eq!(received, Some((b"reply".to_vec(), host)));
        });
}
#[test]
fn stock_relay_ephemeral_tcp_udp_publication_ipv4() {
    ephemeral_publications("127.0.0.1".parse().unwrap());
}
#[test]
fn stock_relay_ephemeral_tcp_udp_publication_ipv6() {
    ephemeral_publications("::1".parse().unwrap());
}
