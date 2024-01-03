use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use super::{hyper_proxy::HyperProxyConnector, socket_manager::SocketManager};

#[derive(Debug, Clone)]
pub(crate) struct DProxyInstance {
    pub(super) last_used: Arc<Mutex<Instant>>,
    pub(super) socket_manager: Arc<SocketManager>,
    pub(super) client: hyper::Client<HyperProxyConnector>,
}
