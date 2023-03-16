use crate::http::HttpClientCapabilityV1;

/// Defines capabilities for a Wasi environment.
#[derive(Clone, Debug)]
pub struct Capabilities {
    pub insecure_allow_all: bool,
    pub http_client: HttpClientCapabilityV1,
    pub threading: CapabilityThreadingV1,
}

impl Capabilities {
    pub fn new() -> Self {
        Self {
            insecure_allow_all: false,
            http_client: Default::default(),
            threading: Default::default(),
        }
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}

/// Defines threading related permissions.
#[derive(Debug, Default, Clone)]
pub struct CapabilityThreadingV1 {
    /// Maximum number of threads that can be spawned.
    ///
    /// [`None`] means no limit.
    pub max_threads: Option<usize>,
}
