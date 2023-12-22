//use wasmer::{Memory, Store};

use crate::WasiEnv;

#[derive(Debug)]
pub(crate) struct DcgiInstance {
    pub env: WasiEnv,
    //pub memory: Memory,
    //pub store: Store,
}
