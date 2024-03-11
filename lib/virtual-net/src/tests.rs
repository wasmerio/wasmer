#![allow(unused)]
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::atomic::{AtomicU16, Ordering},
};

use tracing_test::traced_test;

#[cfg(feature = "remote")]
use crate::RemoteNetworkingServer;
use crate::{
    host::LocalNetworking, meta::FrameSerializationFormat, VirtualConnectedSocketExt,
    VirtualTcpListenerExt,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

#[cfg(feature = "remote")]
async fn setup_mpsc() -> (RemoteNetworkingClient, RemoteNetworkingServer) {
    tracing::info!("building MPSC channels");
    let (tx1, rx1) = tokio::sync::mpsc::channel(100);
    let (tx2, rx2) = tokio::sync::mpsc::channel(100);

    tracing::info!("constructing remote client (mpsc)");
    let (client, client_driver) = RemoteNetworkingClient::new_from_mpsc(tx1, rx2);

    tracing::info!("spawning driver for remote client");
    tokio::task::spawn(client_driver);

    tracing::info!("create local networking provider");
    let local_networking = LocalNetworking::new();

    tracing::info!("constructing remote server (mpsc)");
    let (server, server_driver) =
        RemoteNetworkingServer::new_from_mpsc(tx2, rx1, Arc::new(local_networking));

    tracing::info!("spawning driver for remote server");
    tokio::task::spawn(server_driver);

    (client, server)
}

#[cfg(feature = "remote")]
async fn setup_pipe(
    buf_size: usize,
    format: FrameSerializationFormat,
) -> (RemoteNetworkingClient, RemoteNetworkingServer) {
    tracing::info!("building duplex streams");
    let (tx1, rx1) = tokio::io::duplex(buf_size);
    let (tx2, rx2) = tokio::io::duplex(buf_size);

    tracing::info!("constructing remote client (mpsc)");
    let (client, client_driver) = RemoteNetworkingClient::new_from_async_io(tx1, rx2, format);

    tracing::info!("spawning driver for remote client");
    tokio::task::spawn(client_driver);

    tracing::info!("create local networking provider");
    let local_networking = LocalNetworking::new();

    tracing::info!("constructing remote server (mpsc)");
    let (server, server_driver) =
        RemoteNetworkingServer::new_from_async_io(tx2, rx1, format, Arc::new(local_networking));

    tracing::info!("spawning driver for remote server");
    tokio::task::spawn(server_driver);

    (client, server)
}

#[cfg(feature = "remote")]
async fn test_tcp(client: RemoteNetworkingClient, _server: RemoteNetworkingServer) {
    let mut listener = client
        .listen_tcp(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            false,
            false,
            false,
        )
        .await
        .unwrap();
    let addr: SocketAddr = listener.addr_local().unwrap();
    tracing::info!("listening on {addr}");

    const TEST1: &str = "the cat ran up the wall!";
    const TEST2: &str = "...and fell off the roof! raise the roof! oop oop";

    tracing::info!("spawning acceptor worker thread");
    tokio::task::spawn(async move {
        tracing::info!("waiting for connection");
        let (mut socket, addr) = listener.accept().await.unwrap();
        tracing::info!("accepted connection from {addr}");

        tracing::info!("receiving data from client");
        let mut buf = [0u8; TEST1.len()];
        socket.read_exact(&mut buf).await.unwrap();

        let msg = String::from_utf8_lossy(&buf);
        assert_eq!(msg.as_ref(), TEST1);

        tracing::info!("sending back test string - {TEST2}");
        socket.send(TEST2.as_bytes()).await.unwrap();
    });

    tracing::info!("connecting to listening socket");
    let mut socket = client
        .connect_tcp(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), addr)
        .await
        .unwrap();

    tracing::info!("sending test string - {TEST1}");
    socket.write_all(TEST1.as_bytes()).await.unwrap();

    tracing::info!("receiving data from server");
    let mut buf = [0u8; TEST2.len()];
    socket.read_exact(&mut buf).await.unwrap();

    let msg = String::from_utf8_lossy(&buf);
    assert_eq!(msg.as_ref(), TEST2);

    tracing::info!("all good");
}

#[cfg(feature = "remote")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_mpsc() {
    let (client, server) = setup_mpsc().await;
    test_tcp(client, server).await
}

// Disabled on musl due to flakiness.
// See https://github.com/wasmerio/wasmer/issues/4425
#[cfg(not(target_env = "musl"))]
#[cfg(feature = "remote")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_small_pipe_using_bincode() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::Bincode).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_large_pipe_using_bincode() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::Bincode).await;
    test_tcp(client, server).await
}

// Disabled on musl due to flakiness.
// See https://github.com/wasmerio/wasmer/issues/4425
#[cfg(not(target_env = "musl"))]
#[cfg(feature = "remote")]
#[cfg(feature = "json")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_small_pipe_using_json() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::Json).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "json")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_large_pipe_json_using_json() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::Json).await;
    test_tcp(client, server).await
}

// Disabled on musl due to flakiness.
// See https://github.com/wasmerio/wasmer/issues/4425
#[cfg(not(target_env = "musl"))]
#[cfg(feature = "remote")]
#[cfg(feature = "messagepack")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_small_pipe_using_messagepack() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::MessagePack).await;
    test_tcp(client, server).await
}

// Disabled on musl due to flakiness.
// See https://github.com/wasmerio/wasmer/issues/4425
#[cfg(not(target_env = "musl"))]
#[cfg(feature = "remote")]
#[cfg(feature = "messagepack")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_large_pipe_json_using_messagepack() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::MessagePack).await;
    test_tcp(client, server).await
}

// Disabled on musl due to flakiness.
// See https://github.com/wasmerio/wasmer/issues/4425
#[cfg(not(target_env = "musl"))]
#[cfg(feature = "remote")]
#[cfg(feature = "cbor")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_small_pipe_using_cbor() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::Cbor).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "cbor")]
#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn test_tcp_with_large_pipe_json_using_cbor() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::Cbor).await;
    test_tcp(client, server).await
}

#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test]
#[serial_test::serial]
async fn test_google_poll() {
    use futures_util::Future;

    // Resolve the address
    tracing::info!("resolving www.google.com");
    let networking = LocalNetworking::new();
    let peer_addr = networking
        .resolve("www.google.com", None, None)
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("IP address should be returned");
    tracing::info!("www.google.com = {}", peer_addr);

    // Start the connection
    tracing::info!("connecting to {}:80", peer_addr);
    let mut socket = networking
        .connect_tcp(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            SocketAddr::new(peer_addr, 80),
        )
        .await
        .unwrap();
    tracing::info!("setting nodelay");
    socket.set_nodelay(true).unwrap();
    tracing::info!("setting keepalive");
    socket.set_keepalive(true).unwrap();

    // Wait for it to be ready to send packets
    tracing::info!("waiting for write_ready");
    struct Poller<'a> {
        socket: &'a mut Box<dyn VirtualTcpSocket + Sync>,
    }
    impl<'a> Future for Poller<'a> {
        type Output = Result<usize>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.socket.poll_write_ready(cx)
        }
    }
    Poller {
        socket: &mut socket,
    }
    .await;

    // Send the data (GET http request)
    let data =
        b"GET / HTTP/1.1\r\nHost: www.google.com\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\nConnection: Close\r\n\r\n";
    tracing::info!("sending {} bytes", data.len());
    let sent = socket.send(data).await.unwrap();
    assert_eq!(sent, data.len());

    // Enter a loop that will return all the data
    loop {
        // Wait for the next bit of data
        tracing::info!("waiting for read ready");
        struct Poller<'a> {
            socket: &'a mut Box<dyn VirtualTcpSocket + Sync>,
        }
        impl<'a> Future for Poller<'a> {
            type Output = Result<usize>;
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                self.socket.poll_read_ready(cx)
            }
        }
        Poller {
            socket: &mut socket,
        }
        .await;

        // Now read the data
        let mut buf = [0u8; 4096];
        match socket.read(&mut buf).await {
            Ok(0) => break,
            Ok(amt) => {
                tracing::info!("received {amt} bytes");
                continue;
            }
            Err(err) => {
                tracing::info!("failed - {}", err);
                panic!("failed to receive data");
            }
        }
    }

    tracing::info!("done");
}

#[cfg_attr(windows, ignore)]
#[traced_test]
#[tokio::test]
#[serial_test::serial]
async fn test_google_epoll() {
    use futures_util::Future;
    use virtual_mio::SharedWakerInterestHandler;

    // Resolve the address
    tracing::info!("resolving www.google.com");
    let networking = LocalNetworking::new();
    let peer_addr = networking
        .resolve("www.google.com", None, None)
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("IP address should be returned");
    tracing::info!("www.google.com = {}", peer_addr);

    // Start the connection
    tracing::info!("connecting to {}:80", peer_addr);
    let mut socket = networking
        .connect_tcp(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            SocketAddr::new(peer_addr, 80),
        )
        .await
        .unwrap();
    tracing::info!("setting nodelay");
    socket.set_nodelay(true).unwrap();
    tracing::info!("setting keepalive");
    socket.set_keepalive(true).unwrap();

    // Wait for it to be ready to send packets
    tracing::info!("waiting for writability");
    struct Poller<'a> {
        handler: Option<Box<SharedWakerInterestHandler>>,
        socket: &'a mut Box<dyn VirtualTcpSocket + Sync>,
    }
    impl<'a> Future for Poller<'a> {
        type Output = Result<()>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.handler.is_none() {
                self.handler
                    .replace(SharedWakerInterestHandler::new(cx.waker()));
                let handler = self.handler.as_ref().unwrap().clone();
                self.socket.set_handler(handler);
            }
            if self
                .handler
                .as_mut()
                .unwrap()
                .pop_interest(InterestType::Writable)
            {
                return Poll::Ready(Ok(()));
            }
            Poll::Pending
        }
    }
    Poller {
        handler: None,
        socket: &mut socket,
    }
    .await;

    // Send the data (GET http request)
    let data =
        b"GET / HTTP/1.1\r\nHost: www.google.com\r\nUser-Agent: curl/7.81.0\r\nAccept: */*\r\nConnection: Close\r\n\r\n";
    tracing::info!("sending {} bytes", data.len());
    let sent = socket.try_send(data).unwrap();
    assert_eq!(sent, data.len());

    // We detect if there are lots of false positives, that means something has gone
    // wrong with the epoll implementation
    let mut false_interest = 0usize;

    // Enter a loop that will return all the data
    loop {
        // Wait for the next bit of data
        tracing::info!("waiting for readability");
        struct Poller<'a> {
            handler: Option<Box<SharedWakerInterestHandler>>,
            socket: &'a mut Box<dyn VirtualTcpSocket + Sync>,
        }
        impl<'a> Future for Poller<'a> {
            type Output = Result<()>;
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                if self.handler.is_none() {
                    self.handler
                        .replace(SharedWakerInterestHandler::new(cx.waker()));
                    let handler = self.handler.as_ref().unwrap().clone();
                    self.socket.set_handler(handler);
                }
                if self
                    .handler
                    .as_mut()
                    .unwrap()
                    .pop_interest(InterestType::Readable)
                {
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending
            }
        }
        Poller {
            handler: None,
            socket: &mut socket,
        }
        .await;

        // Now read the data until we block
        let mut done = false;
        for n in 0.. {
            let mut buf: [MaybeUninit<u8>; 4096] = [MaybeUninit::uninit(); 4096];
            match socket.try_recv(&mut buf) {
                Ok(0) => {
                    done = true;
                    break;
                }
                Ok(amt) => {
                    tracing::info!("received {amt} bytes");
                    continue;
                }
                Err(NetworkError::WouldBlock) => {
                    if n == 0 {
                        false_interest += 1;
                    }
                    break;
                }
                Err(err) => {
                    tracing::info!("failed - {}", err);
                    panic!("failed to receive data");
                }
            }
        }
        if done {
            break;
        }
    }

    if false_interest > 20 {
        panic!("too many false positives on the epoll ({false_interest}), something has likely gone wrong")
    }

    tracing::info!("done");
}
