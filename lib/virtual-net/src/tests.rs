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
    static PORT: AtomicU16 = AtomicU16::new(8000);
    let addr = SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::LOCALHOST,
        PORT.fetch_add(1, Ordering::SeqCst),
    ));
    tracing::info!("listening on {addr}");
    let mut listener = client
        .listen_tcp(addr.clone(), false, false, false)
        .await
        .unwrap();

    const TEST1: &'static str = "the cat ran up the wall!";
    const TEST2: &'static str = "...and fell off the roof! raise the roof! oop oop";

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
        .connect_tcp(
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
            addr,
        )
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
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_mpsc() {
    let (client, server) = setup_mpsc().await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_small_pipe_using_bincode() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::Bincode).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_large_pipe_using_bincode() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::Bincode).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "json")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_small_pipe_using_json() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::Json).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "json")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_large_pipe_json_using_json() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::Json).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "messagepack")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_small_pipe_using_messagepack() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::MessagePack).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "messagepack")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_large_pipe_json_using_messagepack() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::MessagePack).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "cbor")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_small_pipe_using_cbor() {
    let (client, server) = setup_pipe(10, FrameSerializationFormat::Cbor).await;
    test_tcp(client, server).await
}

#[cfg(feature = "remote")]
#[cfg(feature = "cbor")]
#[cfg(target_os = "linux")]
#[traced_test]
#[tokio::test]
async fn test_tcp_with_large_pipe_json_using_cbor() {
    let (client, server) = setup_pipe(1024000, FrameSerializationFormat::Cbor).await;
    test_tcp(client, server).await
}
