use bincode::config;

/// A snapshot that captures the runtime state of an instance.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StoreSnapshot {
    /// Values of all globals, indexed by the same index used in Webassembly.
    pub globals: Vec<u128>,
}

impl StoreSnapshot {
    pub fn serialize(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        bincode::serde::encode_to_vec(self, config::legacy())
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, bincode::error::DecodeError> {
        bincode::serde::decode_from_slice(data, config::legacy()).map(|(ret, _)| ret)
    }
}

pub fn capture_store_snapshot(store: &mut impl wasmer::AsStoreMut) -> StoreSnapshot {
    let objs = store.objects_mut();
    let globals = objs.as_u128_globals();
    StoreSnapshot { globals }
}

pub fn restore_store_snapshot(store: &mut impl wasmer::AsStoreMut, snapshot: &StoreSnapshot) {
    let objs = store.objects_mut();

    for (index, value) in snapshot.globals.iter().enumerate() {
        objs.set_global_unchecked(index, *value);
    }
}
