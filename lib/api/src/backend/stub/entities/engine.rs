use wasmer_types::{target::Target, Features};

/// Lightweight engine placeholder used by the stub backend.
#[derive(Clone, Debug, Default)]
pub struct Engine;

impl Engine {
    pub fn new() -> Self {
        Self
    }

    pub fn deterministic_id(&self) -> String {
        "stub".to_string()
    }

    pub fn default_features() -> Features {
        Features::default()
    }

    pub fn supported_features() -> Features {
        Features::default()
    }

    pub fn default_features_for_target(_target: &Target) -> Features {
        Features::default()
    }

    pub fn supported_features_for_target(_target: &Target) -> Features {
        Features::default()
    }
}

pub(crate) fn default_engine() -> Engine {
    Engine::default()
}
