use async_trait::async_trait;
use futures::stream::SplitSink;
use futures::stream::SplitStream;
use futures::SinkExt;
use futures_util::StreamExt;
use wasmer_os::wasmer_wasi::WasiRuntimeImplementation;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use wasmer_os::wasmer_wasi::WebSocketAbi;

#[allow(unused_imports)]
use tracing::{debug, error, info, instrument, span, trace, warn, Level};

pub struct TerminalWebSocket {
    sink: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    stream: Option<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
    on_close: Arc<Mutex<Option<Box<dyn Fn() + Send + 'static>>>>,
}

impl TerminalWebSocket {
    pub async fn new(url: &str) -> Result<TerminalWebSocket, String> {
        let url = url::Url::parse(url)
            .map_err(|err| err.to_string())?;

        let (ws_stream, _) = connect_async(url).await
            .map_err(|err| format!("failed to connect - {}", err))?;
        let (sink, stream) = ws_stream.split();

        Ok(
            TerminalWebSocket {
                sink,
                stream: Some(stream),
                on_close: Arc::new(Mutex::new(None)),
            }
        )
    }
}

#[async_trait]
impl WebSocketAbi for TerminalWebSocket {
    fn set_onopen(&mut self, mut callback: Box<dyn FnMut()>) {
        // We instantly notify that we are open
        callback();
    }

    fn set_onclose(&mut self, callback: Box<dyn Fn() + Send + 'static>) {
        let mut guard = self.on_close.lock().unwrap();
        guard.replace(callback);
    }

    fn set_onmessage(&mut self, callback: Box<dyn Fn(Vec<u8>) + Send + 'static>, runtime: &dyn WasiRuntimeImplementation)
    {
        if let Some(mut stream) = self.stream.take() {
            let on_close = self.on_close.clone();
            runtime.task_shared(Box::new(move || Pin::new(Box::new(async move {
                while let Some(msg) = stream.next().await {
                    match msg {
                        Ok(Message::Binary(msg)) => {
                            callback(msg);
                        }
                        a => {
                            debug!("received invalid msg: {:?}", a);
                        }
                    }
                }
                let on_close = on_close.lock().unwrap();
                if let Some(on_close) = on_close.as_ref() {
                    on_close();
                }
            }))));
        }
    }

    async fn send(&mut self, data: Vec<u8>) -> Result<(), String> {
        self.sink
            .send(Message::binary(data))
            .await
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}
