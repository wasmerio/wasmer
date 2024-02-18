use std::time::Duration;

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

    /// Merges another [`Capabilities`] object into this one, overwriting fields
    /// if necessary.
    pub fn update(&mut self, other: Capabilities) {
        let Capabilities {
            insecure_allow_all,
            http_client,
            threading,
        } = other;
        self.insecure_allow_all |= insecure_allow_all;
        self.http_client.update(http_client);
        self.threading.update(threading);
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

    /// Flag that indicates if asynchronous threading is disabled
    /// (default = false)
    pub enable_asynchronous_threading: bool,

    /// Enables an exponential backoff of the process CPU usage when there
    /// are no active run tokens (when set holds the maximum amount of
    /// time that it will pause the CPU)
    /// (default = off)
    pub enable_exponential_cpu_backoff: Option<Duration>,
}

impl CapabilityThreadingV1 {
    pub fn update(&mut self, other: CapabilityThreadingV1) {
        let CapabilityThreadingV1 {
            max_threads,
            enable_asynchronous_threading,
            enable_exponential_cpu_backoff,
        } = other;
        self.enable_asynchronous_threading |= enable_asynchronous_threading;
        if let Some(val) = enable_exponential_cpu_backoff {
            self.enable_exponential_cpu_backoff = Some(val);
        }
        self.max_threads = max_threads.or(self.max_threads);
    }
}
