use std::time::Duration;

use crate::http::HttpClientCapabilityV1;

/// Defines capabilities for a Wasi environment.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Capabilities {
    pub insecure_allow_all: bool,
    pub http_client: HttpClientCapabilityV1,
    pub polling: CapabilityPollingV1,
    pub max_sock_recv_size: Option<u64>,
    pub threading: CapabilityThreadingV1,
}

impl Capabilities {
    pub fn new() -> Self {
        Self {
            insecure_allow_all: false,
            http_client: Default::default(),
            polling: Default::default(),
            max_sock_recv_size: Some(16 * 1024 * 1024),
            threading: Default::default(),
        }
    }

    /// Merges another [`Capabilities`] object into this one, overwriting fields
    /// if necessary.
    pub fn update(&mut self, other: Capabilities) {
        let Capabilities {
            insecure_allow_all,
            http_client,
            polling,
            max_sock_recv_size,
            threading,
        } = other;
        self.insecure_allow_all |= insecure_allow_all;
        self.http_client.update(http_client);
        self.polling.update(polling);
        self.max_sock_recv_size = max_sock_recv_size.or(self.max_sock_recv_size);
        self.threading.update(threading);
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}

/// Defines polling related permissions and limits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapabilityPollingV1 {
    /// Maximum number of subscriptions accepted by `poll_oneoff`.
    ///
    /// [`None`] means no explicit limit.
    pub max_poll_subscriptions: Option<usize>,
}

impl Default for CapabilityPollingV1 {
    fn default() -> Self {
        Self {
            max_poll_subscriptions: Some(1024),
        }
    }
}

impl CapabilityPollingV1 {
    pub fn update(&mut self, other: CapabilityPollingV1) {
        let CapabilityPollingV1 {
            max_poll_subscriptions,
        } = other;
        self.max_poll_subscriptions = max_poll_subscriptions;
    }
}

/// Defines threading related permissions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapabilityThreadingV1 {
    /// Maximum number of threads that can be spawned.
    ///
    /// [`None`] means no limit.
    pub max_threads: Option<usize>,

    /// Flag that indicates if asynchronous threading is enabled.
    /// (default = true)
    pub enable_asynchronous_threading: bool,

    /// Flag that indicates if deep sleep is enabled.
    /// (default = false)
    pub enable_deep_sleep: bool,

    /// Enables an exponential backoff of the process CPU usage when there
    /// are no active run tokens (when set holds the maximum amount of
    /// time that it will pause the CPU)
    /// (default = off)
    pub enable_exponential_cpu_backoff: Option<Duration>,

    /// Switches to a blocking sleep implementation instead
    /// of the asynchronous runtime based implementation
    pub enable_blocking_sleep: bool,
}

impl Default for CapabilityThreadingV1 {
    fn default() -> Self {
        Self {
            max_threads: None,
            enable_asynchronous_threading: true,
            enable_deep_sleep: false,
            enable_exponential_cpu_backoff: None,
            enable_blocking_sleep: false,
        }
    }
}

impl CapabilityThreadingV1 {
    pub fn update(&mut self, other: CapabilityThreadingV1) {
        let CapabilityThreadingV1 {
            max_threads,
            enable_asynchronous_threading,
            enable_deep_sleep,
            enable_exponential_cpu_backoff,
            enable_blocking_sleep,
        } = other;
        self.enable_asynchronous_threading |= enable_asynchronous_threading;
        self.enable_deep_sleep |= enable_deep_sleep;
        if let Some(val) = enable_exponential_cpu_backoff {
            self.enable_exponential_cpu_backoff = Some(val);
        }
        self.max_threads = max_threads.or(self.max_threads);
        self.enable_blocking_sleep |= enable_blocking_sleep;
    }
}
