use std::sync::Arc;

use crate::runners::dproxy::socket_manager::SocketManager;

use super::*;

#[derive(Debug)]
pub struct HyperProxyConnectorBuilder {
    socket_manager: Arc<SocketManager>,
}

impl HyperProxyConnectorBuilder {
    pub fn new(socket_manager: Arc<SocketManager>) -> Self {
        Self { socket_manager }
    }

    pub async fn build(self) -> HyperProxyConnector {
        HyperProxyConnector {
            socket_manager: self.socket_manager,
        }
    }
}
