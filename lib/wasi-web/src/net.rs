use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use tokio::sync::mpsc;
use virtual_net::{meta::MessageRequest, RemoteNetworkingClient};
use wasm_bindgen_futures::JsFuture;

use crate::{runtime::bindgen_sleep, ws::WebSocket};

pub fn connect_networking(connect: String) -> RemoteNetworkingClient {
    let (recv_tx, recv_rx) = mpsc::channel(100);
    let (send_tx, send_rx) = mpsc::channel(100);
    let send_tx2 = send_tx.clone();

    let (client, driver) = virtual_net::RemoteNetworkingClient::new_from_mpsc(send_tx, recv_rx);
    wasm_bindgen_futures::spawn_local(driver);

    let send_rx = Arc::new(tokio::sync::Mutex::new(send_rx));

    wasm_bindgen_futures::spawn_local(async move {
        let backoff = Arc::new(AtomicUsize::new(0));
        loop {
            // Exponential backoff prevents thrashing of the connection
            let backoff_ms = backoff.load(Ordering::SeqCst);
            if backoff_ms > 0 {
                let promise = bindgen_sleep(backoff_ms as i32);
                JsFuture::from(promise).await.ok();
            }
            let new_backoff = 8000usize.min((backoff_ms * 2) + 100);
            backoff.store(new_backoff, Ordering::SeqCst);

            // Establish a websocket connection to the edge network
            let mut ws = match WebSocket::new(connect.as_str()) {
                Ok(ws) => ws,
                Err(err) => {
                    tracing::error!("failed to establish web socket connection - {}", err);
                    continue;
                }
            };

            // Wire up the events
            let (relay_tx, mut relay_rx) = mpsc::unbounded_channel();
            let (connected_tx, mut connected_rx) = mpsc::unbounded_channel();
            ws.set_onopen({
                let connect = connect.clone();
                let connected_tx = connected_tx.clone();
                Box::new(move || {
                    tracing::debug!(url = connect, "networking web-socket opened");
                    connected_tx.send(true).ok();
                })
            });
            ws.set_onclose({
                let connect = connect.clone();

                let connected_tx = connected_tx.clone();
                let relay_tx = relay_tx.clone();
                Box::new(move || {
                    tracing::debug!(url = connect, "networking web-socket closed");
                    relay_tx.send(Vec::new()).ok();
                    connected_tx.send(false).ok();
                })
            });
            ws.set_onmessage({
                Box::new(move |data| {
                    relay_tx.send(data).unwrap();
                })
            });

            // Wait for it to connect and setup the rest of the callbacks
            if !connected_rx.recv().await.unwrap_or_default() {
                continue;
            }
            backoff.store(100, Ordering::SeqCst);

            // We process any backends
            wasm_bindgen_futures::spawn_local({
                let send_tx2 = send_tx2.clone();
                let recv_tx = recv_tx.clone();
                async move {
                    while let Some(data) = relay_rx.recv().await {
                        if data.is_empty() {
                            break;
                        }
                        let data = match bincode::deserialize(&data) {
                            Ok(d) => d,
                            Err(err) => {
                                tracing::error!(
                                    "failed to deserialize networking message - {}",
                                    err
                                );
                                break;
                            }
                        };
                        if recv_tx.send(data).await.is_err() {
                            break;
                        }
                    }
                    send_tx2.try_send(MessageRequest::Reconnect).ok();
                }
            });

            while let Some(data) = send_rx.lock().await.recv().await {
                if let MessageRequest::Reconnect = &data {
                    tracing::info!("websocket will reconnect");
                    break;
                }
                let data = match bincode::serialize(&data) {
                    Ok(d) => d,
                    Err(err) => {
                        tracing::error!("failed to serialize networking message - {}", err);
                        break;
                    }
                };
                if let Err(err) = ws.send(data) {
                    tracing::error!("websocket has failed - {}", err);
                    break;
                }
            }
        }
    });
    client
}
