use wasmer::Store;

use crate::WasiFunctionEnv;

#[derive(Debug)]
pub(crate) struct DcgiInstance {
    pub env: WasiFunctionEnv,
    pub store: Store,
}
