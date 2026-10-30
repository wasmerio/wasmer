//! The historical WAI HTTP extension. Layout authority: Wasmer 6ff2f3c194d^,
//! `lib/wasix/src/bindings/wasix_http_client_v1.rs` (removed from v1 in 2024).
//! This is an embedder capability, not a Linux syscall or a package downloader.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use wasixv2_syscalls::LocalFuture;
use wasmer::RuntimeError;

pub const NAMESPACE: &str = "wasix_http_client_v1";
pub const CANONICAL: &str = "canonical_abi";

/// An owned request: no guest borrow or worker-local JS object crosses the provider boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: Option<u32>,
    pub redirect_policy: Option<RedirectPolicy>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirectPolicy {
    NoFollow,
    Follow { max: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
    pub redirect_urls: Option<Vec<String>>,
}

/// Explicit HTTP capability. The embedder owns URL/header policy, Fetch and CORS behavior.
/// On the blocking browser lane, a future must never depend on its task worker's JS event
/// loop; dispatch Promise-producing I/O to the coordinator/I/O worker before parking.
/// Dropping a send future must cancel/release its in-flight operation.
pub trait HttpProvider: Send + Sync {
    fn new_client(&self) -> Result<(), String> {
        Ok(())
    }
    fn send(&self, request: Request) -> LocalFuture<'_, Result<Response, String>>;
}

#[derive(Default)]
struct Clients {
    next: u32,
    live: HashSet<u32>,
}

/// Like the old generated binding, resource tables are instance-local and new on fork.
pub(crate) struct State {
    provider: Option<Arc<dyn HttpProvider>>,
    clients: Mutex<Clients>,
}

impl State {
    pub(crate) fn new(provider: Option<Arc<dyn HttpProvider>>) -> Self {
        Self {
            provider,
            clients: Mutex::default(),
        }
    }

    pub(crate) fn create(&self) -> Result<u32, String> {
        let provider = self.provider.as_ref().ok_or("No http client available")?;
        provider.new_client()?;
        let mut clients = self.clients.lock().unwrap();
        let handle = clients.next;
        clients.next = handle
            .checked_add(1)
            .ok_or("HTTP client handles exhausted")?;
        clients.live.insert(handle);
        Ok(handle)
    }

    pub(crate) fn provider(&self, handle: u32) -> Result<Arc<dyn HttpProvider>, RuntimeError> {
        if !self.clients.lock().unwrap().live.contains(&handle) {
            return Err(invalid("client handle"));
        }
        self.provider
            .clone()
            .ok_or_else(|| invalid("client provider"))
    }

    pub(crate) fn remove(&self, handle: u32) -> Result<(), RuntimeError> {
        if !self.clients.lock().unwrap().live.remove(&handle) {
            return Err(invalid("client handle"));
        }
        Ok(())
    }
}

pub(crate) fn invalid(what: &str) -> RuntimeError {
    RuntimeError::new(format!("invalid WASIX v1 HTTP {what}"))
}
