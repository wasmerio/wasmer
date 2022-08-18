#[cfg(feature = "async_ws")]
use async_trait::async_trait;

use crate::WasiRuntimeImplementation;

// This ABI implements a general purpose web socket
#[cfg_attr(feature = "async_ws", async_trait)]
pub trait WebSocketAbi {
    fn set_onopen(&mut self, callback: Box<dyn FnMut()>);

    fn set_onclose(&mut self, callback: Box<dyn Fn() + Send + 'static>);

    fn set_onmessage(&mut self, callback: Box<dyn Fn(Vec<u8>) + Send + 'static>, runtime: &dyn WasiRuntimeImplementation);

    #[cfg(feature = "async_ws")]
    async fn send(&mut self, data: Vec<u8>) -> Result<(), String>;

    #[cfg(not(feature = "async_ws"))]
    fn send(&mut self, data: Vec<u8>) -> Result<(), String>;
}
