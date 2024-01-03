use std::sync::Arc;

use crate::runners::dproxy::socket_manager::SocketManager;

use super::*;

#[derive(Debug)]
pub struct HyperProxyConnectorBuilder {
    socket_manager: Arc<SocketManager>,
    reuse: bool,
}

impl HyperProxyConnectorBuilder {
    pub fn new(socket_manager: Arc<SocketManager>) -> Self {
        Self {
            socket_manager,
            reuse: true,
        }
    }

    pub fn with_reuse(mut self, reuse: bool) -> Self {
        self.reuse = reuse;
        self
    }

    pub async fn build(self) -> HyperProxyConnector {
        HyperProxyConnector {
            socket_manager: self.socket_manager,
            reuse: self.reuse,
        }
    }
}
