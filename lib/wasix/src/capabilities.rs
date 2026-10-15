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

    /// Whether the guest is entered asynchronously: whether its start function,
    /// and the entry function of every thread it spawns and every fork it
    /// makes, is called with `Function::call_async` rather than
    /// `Function::call`.
    ///
    /// Only a guest entered asynchronously can suspend. A guest suspends when it
    /// calls an asynchronous host import (one registered with
    /// `Function::new_*_async`) and that import has to wait. The WASIX
    /// context-switching syscalls are such imports, and so can be those of
    /// other host APIs: N-API's event loop checkpoint on the JS backend is one.
    /// A guest entered synchronously that reaches an asynchronous import gets an
    /// error instead.
    ///
    /// Turn it off when nothing the guest imports needs to suspend, and you
    /// would rather not pay for the machinery: on `sys` an asynchronous entry
    /// runs the guest on a coroutine with a stack of its own, and on JS it goes
    /// through `WebAssembly.promising`. Turning it off also turns off
    /// [`Self::enable_context_switching`], which cannot work without it; see
    /// [`Self::context_switching_enabled`].
    ///
    /// Engines without async support always enter synchronously, as does a guest
    /// instrumented with Asyncify on the JS backend, where Asyncify and JSPI are
    /// alternative mechanisms that must not be combined.
    ///
    /// # The three configurations
    ///
    /// | `enable_async_entrypoint` | `enable_context_switching` | who it is for |
    /// |---|---|---|
    /// | on  | on  | the default: any guest that does not use N-API |
    /// | on  | off | N-API guests on the JS backend |
    /// | off | (ignored) | N-API guests on `sys`, through V8 |
    ///
    /// **Both on.** The guest gets the context-switching API (`context_create`,
    /// `context_switch`, …), and on the JS backend `call_dynamic` and the
    /// dynamic linker's lazy-binding stubs re-enter the guest asynchronously, so
    /// that code they call into may suspend in turn. Limitations:
    /// - A host frame that calls back into the guest synchronously cannot have a
    ///   suspending guest beneath it on JS: `Reflect.apply` is a JavaScript
    ///   frame, and JSPI cannot suspend across one. So on JS most syscalls do
    ///   not run a signal handler when they notice a signal; it waits until the
    ///   guest reaches an asynchronous syscall (`call_dynamic`, or a
    ///   lazy-binding stub), where it may suspend. The exception is a blocking
    ///   wait that the signal interrupts, which runs the handler inline, so a
    ///   guest that never reaches an asynchronous syscall (most statically
    ///   linked guests) still gets its handlers run, but only when it blocks,
    ///   and a handler run there that tries to suspend fails.
    /// - On JS, an asynchronous re-entry costs a JSPI suspension, measured at
    ///   roughly a quarter of a microsecond on top of a plain call.
    /// - It cannot be combined with N-API, whose callbacks re-enter the guest
    ///   through a synchronous boundary. See [`Self::enable_context_switching`].
    ///
    /// **Async entry, no context switching.** Host APIs whose imports suspend at
    /// the top of the guest's stack keep working, but nothing re-enters the
    /// guest asynchronously. Limitations:
    /// - The context-switching syscalls answer `Notsup`.
    /// - `call_dynamic` and the lazy-binding stubs call the guest synchronously,
    ///   so code reached through them must not suspend. On JS, an asynchronous
    ///   import called from beneath any synchronous re-entry is refused and the
    ///   guest sees an error; on `sys`, which can suspend there, it works.
    /// - Signal handlers run inline again, from the syscall that notices the
    ///   signal, which is safe because nothing beneath them can suspend.
    ///
    /// **Sync entry.** Nothing in the guest can suspend at all. Limitations:
    /// - Every asynchronous import fails when called, and the context-switching
    ///   API is unavailable whatever [`Self::enable_context_switching`] says.
    /// - On JS this rules out N-API, whose event loop checkpoint must suspend.
    ///
    /// Per process. The runtime's instantiation hooks may adjust it for each
    /// main module (see `InstantiationHook::configure_capabilities`), and that
    /// adjustment is not inherited: every process, thread and fork starts again
    /// from the capabilities it was given.
    /// (default = true)
    pub enable_async_entrypoint: bool,

    /// Whether the guest may use the WASIX context-switching API, and the
    /// asynchronous guest re-entry it relies on.
    ///
    /// Context switching lets a guest create stacks of its own and switch
    /// between them (`context_create`, `context_switch`, …): green threads,
    /// coroutines, Python's greenlet. Each switch suspends the running stack and
    /// resumes another, so anything the guest can reach while switching must be
    /// able to suspend too. That is why, on the JS backend, `call_dynamic` and
    /// the dynamic linker's lazy-binding stubs re-enter the guest
    /// asynchronously when this is on: a switch can happen in code they called.
    ///
    /// It requires [`Self::enable_async_entrypoint`]: a guest entered
    /// synchronously cannot suspend at all. Read it through
    /// [`Self::context_switching_enabled`], which accounts for that.
    ///
    /// Turn it off for a guest that calls back in through a synchronous
    /// foreign boundary. N-API is one: its bridge invokes guest callbacks
    /// through a C function returning `u32`, so a callback cannot suspend, and
    /// an asynchronous re-entry beneath one would try to. N-API's runtime hooks
    /// turn it off for every main module that imports N-API, through
    /// `InstantiationHook::configure_capabilities`.
    ///
    /// With it off, `context_switch` answers `Notsup`, `context_create` finds no
    /// environment to join, and `call_dynamic` and the lazy-binding stubs call
    /// the guest synchronously. The limitations of each configuration are listed
    /// under [`Self::enable_async_entrypoint`].
    ///
    /// Per process, so one process tree may mix guests that differ: a shell
    /// without N-API can run `node`, and `node` can spawn Python, which keeps
    /// context switching. An adjustment made for one main module is not
    /// inherited; see [`Self::enable_async_entrypoint`]. A single guest cannot
    /// have both, and a side module always follows its main module's choice.
    /// (default = true)
    pub enable_context_switching: bool,
}

impl Capabilities {
    pub fn new() -> Self {
        Self {
            insecure_allow_all: false,
            http_client: Default::default(),
            polling: Default::default(),
            max_sock_recv_size: Some(16 * 1024 * 1024),
            threading: Default::default(),
            enable_async_entrypoint: true,
            enable_context_switching: true,
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
            enable_async_entrypoint,
            enable_context_switching,
        } = other;
        self.insecure_allow_all |= insecure_allow_all;
        self.http_client.update(http_client);
        self.polling.update(polling);
        self.max_sock_recv_size = max_sock_recv_size.or(self.max_sock_recv_size);
        self.threading.update(threading);
        self.enable_async_entrypoint &= enable_async_entrypoint;
        self.enable_context_switching &= enable_context_switching;
    }

    /// Whether the guest gets the context-switching API: both
    /// [`Self::enable_context_switching`] and the
    /// [`Self::enable_async_entrypoint`] it requires.
    pub fn context_switching_enabled(&self) -> bool {
        self.enable_context_switching && self.enable_async_entrypoint
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct CapabilityThreadingV1 {
    /// Maximum number of threads that can be spawned.
    ///
    /// [`None`] means no limit.
    pub max_threads: Option<usize>,

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

impl CapabilityThreadingV1 {
    pub fn update(&mut self, other: CapabilityThreadingV1) {
        let CapabilityThreadingV1 {
            max_threads,
            enable_deep_sleep,
            enable_exponential_cpu_backoff,
            enable_blocking_sleep,
        } = other;
        self.enable_deep_sleep |= enable_deep_sleep;
        if let Some(val) = enable_exponential_cpu_backoff {
            self.enable_exponential_cpu_backoff = Some(val);
        }
        self.max_threads = max_threads.or(self.max_threads);
        self.enable_blocking_sleep |= enable_blocking_sleep;
    }
}
