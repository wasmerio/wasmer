use wasmer_types::StoreId;

use crate::BackendStore;

/// Minimal store objects implementation for the stub backend.
#[derive(Debug, Clone)]
pub struct StoreObjects {
    id: StoreId,
}

impl Default for StoreObjects {
    fn default() -> Self {
        Self { id: StoreId::default() }
    }
}

impl StoreObjects {
    pub fn id(&self) -> StoreId {
        self.id
    }

    pub(crate) fn from_store_ref(store: &BackendStore) -> Self {
        match store {
            BackendStore::Stub(s) => Self { id: s.id() },
            _ => Self::default(),
        }
    }

    pub fn as_u128_globals(&self) -> Vec<u128> {
        Vec::new()
    }

    pub fn set_global_unchecked(&self, _idx: usize, _val: u128) {}
}
