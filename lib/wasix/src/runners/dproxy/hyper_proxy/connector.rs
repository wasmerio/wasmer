use std::sync::Arc;

use tokio_stream::wrappers::BroadcastStream;

use super::socket_manager::SocketManager;

use super::*;

/// A Connector for the WASM processes behind a socket.
#[derive(Debug, Clone)]
pub struct HyperProxyConnector {
    pub(super) socket_manager: Arc<SocketManager>,
}

impl HyperProxyConnector {
    pub fn shutdown(&self) {
        self.socket_manager.shutdown();
    }
}

impl Service<Uri> for HyperProxyConnector {
    type Response = HyperProxyStream;
    type Error = BoxError;

    #[allow(clippy::type_complexity)]
    type Future = Pin<Box<dyn Future<Output = Result<HyperProxyStream, BoxError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _dst: Uri) -> Self::Future {
        let this = self.clone();
        Box::pin(async move {
            let terminate_rx = this.socket_manager.terminate_rx();
            let socket = this.socket_manager.acquire_http_socket().await?;
            let (tx, rx) = socket.split();
            Ok(HyperProxyStream {
                tx,
                rx,
                terminate: BroadcastStream::new(terminate_rx),
                terminated: false,
            })
        })
    }
}
