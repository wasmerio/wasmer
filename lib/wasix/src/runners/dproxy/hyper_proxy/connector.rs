use std::sync::Arc;

use super::socket_manager::SocketManager;

use super::*;

/// A Connector for the WASM processes behind a socket.
#[derive(Debug, Clone)]
pub struct HyperProxyConnector {
    pub(super) socket_manager: Arc<SocketManager>,
    pub(super) reuse: bool,
}

impl HyperProxyConnector {
    pub fn socket_manager(&self) -> &Arc<SocketManager> {
        &self.socket_manager
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
            let socket = this.socket_manager.acquire_http_socket(this.reuse).await?;
            let (tx, rx) = socket.split();
            Ok(HyperProxyStream { tx, rx })
        })
    }
}
