use serde::{Deserialize, Serialize};
use wasm_common::{DataInitializer, DataInitializerLocation};


/// Similar to `DataInitializer`, but owns its own copy of the data rather
/// than holding a slice of the original module.
#[derive(Clone, Serialize, Deserialize)]
pub struct OwnedDataInitializer {
    /// The location where the initialization is to be performed.
    pub location: DataInitializerLocation,

    /// The initialization data.
    pub data: Box<[u8]>,
}

impl OwnedDataInitializer {
    pub fn new(borrowed: &DataInitializer<'_>) -> Self {
        Self {
            location: borrowed.location.clone(),
            data: borrowed.data.to_vec().into_boxed_slice(),
        }
    }
}
