//! The historical WAI HTTP extension. Layout authority: Wasmer 6ff2f3c194d^,
//! `lib/wasix/src/bindings/wasix_http_client_v1.rs` (removed from v1 in 2024).
//! This is an embedder capability, not a Linux syscall or a package downloader.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use wasixv2_exec::{ExecEnv, call_guest};
use wasixv2_syscalls::{Control, LocalFuture, Syscalls, UserMemory};
use wasmer::{Function, FunctionEnv, Imports, Module, RuntimeError, Store, Value};

use crate::shim::{self, Done, Env, Frame};

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

    fn create(&self) -> Result<u32, String> {
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

    fn provider(&self, handle: u32) -> Result<Arc<dyn HttpProvider>, RuntimeError> {
        if !self.clients.lock().unwrap().live.contains(&handle) {
            return Err(invalid("client handle"));
        }
        self.provider
            .clone()
            .ok_or_else(|| invalid("client provider"))
    }

    fn remove(&self, handle: u32) -> Result<(), RuntimeError> {
        if !self.clients.lock().unwrap().live.remove(&handle) {
            return Err(invalid("client handle"));
        }
        Ok(())
    }
}

fn invalid(what: &str) -> RuntimeError {
    RuntimeError::new(format!("invalid WASIX v1 HTTP {what}"))
}

/// ABI signatures shared by import validation and binding.
pub(crate) fn params(module: &str, name: &str) -> Option<usize> {
    match (module, name) {
        (NAMESPACE, "client::new") | (CANONICAL, "resource_drop_client") => Some(1),
        (NAMESPACE, "client::send") => Some(2),
        _ => None,
    }
}

pub(crate) fn imports<S: Syscalls + Control + Send + Sync + 'static>(
    imports: &mut Imports,
    store: &mut Store,
    env: &FunctionEnv<ExecEnv<S>>,
    module: &Module,
) {
    for import in module.imports().functions() {
        let function = match (import.module(), import.name()) {
            (NAMESPACE, "client::new") => {
                Function::new_typed_with_env_async(store, env, client_new::<S>)
            }
            (NAMESPACE, "client::send") => {
                Function::new_typed_with_env_async(store, env, client_send::<S>)
            }
            (CANONICAL, "resource_drop_client") => {
                Function::new_typed_with_env_async(store, env, client_drop::<S>)
            }
            _ => continue,
        };
        imports.define(import.module(), import.name(), function);
    }
}

/// WAI imports have no errno result: bad pointers/variants trap, provider errors use the
/// expected<string> result. Completion still runs through v2's ordinary control path.
fn done(result: Done<Result<(), RuntimeError>>) -> Result<(), RuntimeError> {
    match result {
        Done::Ok(result) => result,
        Done::Err(errno) => Err(RuntimeError::new(format!(
            "WASIX v1 HTTP errno {}",
            errno.0
        ))),
        Done::Switched(_) | Done::Unwinding => Ok(()),
    }
}

async fn client_new<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: i32,
) -> Result<(), RuntimeError> {
    done(
        shim::call(&env, async |frame| {
            Ok(async {
                match frame.instance().http.create() {
                    Ok(handle) => {
                        let mut record = [0; 12];
                        put(&mut record, 4, handle);
                        write(frame.mem.guest(), out as u32, &record).await
                    }
                    Err(error) => write_error(frame, out as u32, error).await,
                }
            }
            .await)
        })
        .await?,
    )
}

async fn client_drop<S: Syscalls + Control + 'static>(
    env: Env<S>,
    handle: i32,
) -> Result<(), RuntimeError> {
    done(
        shim::call(&env, async |frame| {
            Ok(frame.instance().http.remove(handle as u32))
        })
        .await?,
    )
}

async fn client_send<S: Syscalls + Control + 'static>(
    env: Env<S>,
    input: i32,
    out: i32,
) -> Result<(), RuntimeError> {
    // Internal task-work restarts must not replay an HTTP request (especially a POST).
    let mut request = None;
    let result = shim::call(&env, async |frame| {
        if request.is_none() {
            match prepare(frame, input as u32).await {
                Ok(prepared) => request = Some(prepared),
                Err(error) => return Ok(Err(error)),
            }
        }
        let response = wait_response(frame, request.as_mut().unwrap()).await?;
        request.take();
        Ok(match response {
            Ok(response) => write_response(frame, out as u32, response).await,
            Err(error) => write_error(frame, out as u32, error).await,
        })
    })
    .await?;
    // A fatal signal unwinds above; a handled interruption is a WAI error, not a wasm trap.
    drop(request);
    match result {
        Done::Err(errno) => {
            let error = if errno == crate::abi::Errno::INTR {
                "HTTP request interrupted".to_owned()
            } else {
                format!("HTTP request wait failed (errno {})", errno.0)
            };
            done(
                shim::call(&env, async |frame| {
                    Ok(write_error(frame, out as u32, error.clone()).await)
                })
                .await?,
            )
        }
        result => done(result),
    }
}

async fn prepare<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    input: u32,
) -> Result<LocalFuture<'static, Result<Response, String>>, RuntimeError> {
    let bytes = read(frame.mem.guest(), input, 68).await?;
    let provider = frame.instance().http.provider(word(&bytes, 0))?;
    let request = decode(frame.mem.guest(), &bytes).await?;
    Ok(Box::pin(async move {
        match request {
            Ok(request) => provider.send(request).await,
            Err(error) => Err(error),
        }
    }))
}

async fn wait_response<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    request: &mut LocalFuture<'static, Result<Response, String>>,
) -> crate::errno::V1Result<Result<Response, String>> {
    use std::{future::poll_fn, pin::pin, task::Poll};
    use wasixv2_abi::{GuestPtr, GuestPtrMut};
    let caller = frame.caller();
    // Linux fs/select.c:do_sys_poll: no fds and no timeout waits only for a signal or
    // task-work interruption. Use its existing wait registration and restart policy;
    // dropping this future removes the registration when the provider completes first.
    let mut interrupted = pin!(frame.syscalls.ppoll_time64(
        &caller,
        GuestPtrMut::NULL,
        0,
        GuestPtrMut::NULL,
        GuestPtr::NULL,
        8,
    ));
    poll_fn(|cx| {
        if let Poll::Ready(response) = request.as_mut().poll(cx) {
            return Poll::Ready(Ok(response));
        }
        match interrupted.as_mut().poll(cx) {
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
            Poll::Ready(Ok(_)) => Poll::Ready(Err(wasixv2_abi::Errno::EIO.into())),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

async fn read(mem: &dyn UserMemory, addr: u32, len: u32) -> Result<Vec<u8>, RuntimeError> {
    if len != 0 && u64::from(addr) + u64::from(len) > mem.size().await {
        return Err(invalid("memory range"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len as usize)
        .map_err(|_| invalid("allocation size"))?;
    bytes.resize(len as usize, 0);
    mem.read(addr, &mut bytes)
        .await
        .map_err(|_| invalid("memory range"))?;
    Ok(bytes)
}

async fn write(mem: &dyn UserMemory, addr: u32, bytes: &[u8]) -> Result<(), RuntimeError> {
    mem.write(addr, bytes)
        .await
        .map_err(|_| invalid("memory range"))
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

async fn string(mem: &dyn UserMemory, bytes: &[u8], at: usize) -> Result<String, RuntimeError> {
    String::from_utf8(read(mem, word(bytes, at), word(bytes, at + 4)).await?)
        .map_err(|_| invalid("UTF-8 string"))
}

fn present(tag: u8) -> Result<bool, RuntimeError> {
    match tag {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid("option tag")),
    }
}

/// Returns WAI errors for historical unsupported request features; malformed ABI values trap.
async fn decode(
    mem: &dyn UserMemory,
    bytes: &[u8],
) -> Result<Result<Request, String>, RuntimeError> {
    let url = string(mem, bytes, 4).await?;
    let method = match bytes[12] {
        tag @ 0..=8 => [
            "GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH",
        ][tag as usize]
            .to_owned(),
        9 => {
            return Ok(Err(format!(
                "Unknown method: {}",
                string(mem, bytes, 16).await?
            )));
        }
        _ => return Err(invalid("method tag")),
    };
    let count = word(bytes, 28);
    let size = count
        .checked_mul(16)
        .ok_or_else(|| invalid("header list"))?;
    let records = read(mem, word(bytes, 24), size).await?;
    let mut headers = Vec::new();
    for header in records.chunks_exact(16) {
        headers.push((
            string(mem, header, 0).await?,
            read(mem, word(header, 8), word(header, 12)).await?,
        ));
    }
    let body = if present(bytes[32])? {
        match bytes[36] {
            0 => Some(read(mem, word(bytes, 40), word(bytes, 44)).await?),
            1 => return Ok(Err("File descriptor bodies not supported yet".to_owned())),
            _ => return Err(invalid("body tag")),
        }
    } else {
        None
    };
    let timeout_ms = if present(bytes[48])? {
        Some(word(bytes, 52))
    } else {
        None
    };
    let redirect_policy = if present(bytes[56])? {
        Some(match bytes[60] {
            0 => RedirectPolicy::NoFollow,
            1 => RedirectPolicy::Follow {
                max: word(bytes, 64),
            },
            _ => return Err(invalid("redirect policy tag")),
        })
    } else {
        None
    };
    Ok(Ok(Request {
        url,
        method,
        headers,
        body,
        timeout_ms,
        redirect_policy,
    }))
}

async fn allocate<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    bytes: &[u8],
    align: u32,
) -> Result<u32, RuntimeError> {
    let len = u32::try_from(bytes.len()).map_err(|_| invalid("allocation size"))?;
    let realloc = frame
        .instance()
        .realloc
        .as_ref()
        .ok_or_else(|| RuntimeError::new("WASIX v1 HTTP requires canonical_abi_realloc"))?;
    let result = call_guest(
        frame.env,
        realloc,
        &[
            Value::I32(0),
            Value::I32(0),
            Value::I32(align as i32),
            Value::I32(len as i32),
        ],
    )
    .await?;
    let [Value::I32(addr)] = &*result else {
        return Err(invalid("canonical_abi_realloc signature"));
    };
    let addr = *addr as u32;
    if !addr.is_multiple_of(align) {
        return Err(invalid("canonical_abi_realloc alignment"));
    }
    write(frame.mem.guest(), addr, bytes).await?;
    Ok(addr)
}

async fn pair<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    record: &mut [u8],
    at: usize,
    bytes: &[u8],
) -> Result<(), RuntimeError> {
    put(record, at, allocate(frame, bytes, 1).await?);
    put(record, at + 4, bytes.len() as u32);
    Ok(())
}

async fn write_error<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    out: u32,
    error: String,
) -> Result<(), RuntimeError> {
    let mut record = [0; 12];
    record[0] = 1;
    pair(frame, &mut record, 4, error.as_bytes()).await?;
    write(frame.mem.guest(), out, &record).await
}

async fn write_response<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    out: u32,
    response: Response,
) -> Result<(), RuntimeError> {
    let mut record = [0; 40];
    record[4..6].copy_from_slice(&response.status.to_le_bytes());
    let size = response
        .headers
        .len()
        .checked_mul(16)
        .ok_or_else(|| invalid("header list"))?;
    let mut headers = vec![0; size];
    for (slot, (name, value)) in headers.chunks_exact_mut(16).zip(&response.headers) {
        pair(frame, slot, 0, name.as_bytes()).await?;
        pair(frame, slot, 8, value).await?;
    }
    put(&mut record, 8, allocate(frame, &headers, 4).await?);
    put(&mut record, 12, response.headers.len() as u32);
    pair(frame, &mut record, 20, &response.body).await?;
    if let Some(urls) = response.redirect_urls {
        record[28] = 1;
        let size = urls
            .len()
            .checked_mul(8)
            .ok_or_else(|| invalid("redirect list"))?;
        let mut redirects = vec![0; size];
        for (slot, url) in redirects.chunks_exact_mut(8).zip(&urls) {
            pair(frame, slot, 0, url.as_bytes()).await?;
        }
        put(&mut record, 32, allocate(frame, &redirects, 4).await?);
        put(&mut record, 36, urls.len() as u32);
    }
    write(frame.mem.guest(), out, &record).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use wasixv2_syscalls::Fault;

    // Same flat-memory double as syscalls/src/memory.rs; every read/write stays checked.
    struct Flat(RefCell<Vec<u8>>);
    impl UserMemory for Flat {
        fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            Box::pin(async move {
                let memory = self.0.borrow();
                let src = memory
                    .get(addr as usize..addr as usize + buf.len())
                    .ok_or(Fault)?;
                buf.copy_from_slice(src);
                Ok(())
            })
        }
        fn write<'a>(&'a self, addr: u32, bytes: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            Box::pin(async move {
                let mut memory = self.0.borrow_mut();
                let dst = memory
                    .get_mut(addr as usize..addr as usize + bytes.len())
                    .ok_or(Fault)?;
                dst.copy_from_slice(bytes);
                Ok(())
            })
        }
        fn size(&self) -> LocalFuture<'_, u64> {
            Box::pin(async { self.0.borrow().len() as u64 })
        }
        fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
            Box::pin(async { None })
        }
    }

    struct Mock;
    impl HttpProvider for Mock {
        fn send(&self, _: Request) -> LocalFuture<'_, Result<Response, String>> {
            Box::pin(async { Err("CORS/fetch failure".to_owned()) })
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn resources_require_a_provider_and_reject_stale_handles() {
        assert_eq!(
            State::new(None).create(),
            Err("No http client available".to_owned())
        );
        let state = State::new(Some(Arc::new(Mock)));
        let first = state.create().unwrap();
        assert!(state.provider(first).is_ok());
        state.remove(first).unwrap();
        assert!(state.provider(first).is_err());
        struct Denied;
        impl HttpProvider for Denied {
            fn new_client(&self) -> Result<(), String> {
                Err("Permission denied - http client not enabled".to_owned())
            }
            fn send(&self, _: Request) -> LocalFuture<'_, Result<Response, String>> {
                panic!("denied clients must never send")
            }
        }
        assert!(State::new(Some(Arc::new(Denied))).create().is_err());
        assert!(state.remove(first).is_err());
        let second = state.create().unwrap();
        assert_ne!(first, second);
        assert!(state.provider(first).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn historical_request_offsets_preserve_binary_body_and_options() {
        futures::executor::block_on(async {
            let mem = Flat(RefCell::new(vec![0; 512]));
            let mut record = [0; 68];
            write(&mem, 128, b"https://fixture.test/").await.unwrap();
            put(&mut record, 4, 128);
            put(&mut record, 8, 21);
            record[12] = 2; // POST
            put(&mut record, 24, 192);
            put(&mut record, 28, 1);
            let mut header = [0; 16];
            put(&mut header, 0, 224);
            put(&mut header, 4, 1);
            put(&mut header, 8, 225);
            put(&mut header, 12, 1);
            write(&mem, 192, &header).await.unwrap();
            write(&mem, 224, b"xy").await.unwrap();
            write(&mem, 240, &[0, 128, 255]).await.unwrap();
            record[32] = 1;
            put(&mut record, 40, 240);
            put(&mut record, 44, 3);
            record[48] = 1;
            put(&mut record, 52, 123);
            record[56] = 1;
            record[60] = 1;
            put(&mut record, 64, 7);
            let request = decode(&mem, &record).await.unwrap().unwrap();
            assert_eq!(
                request,
                Request {
                    url: "https://fixture.test/".to_owned(),
                    method: "POST".to_owned(),
                    headers: vec![("x".to_owned(), b"y".to_vec())],
                    body: Some(vec![0, 128, 255]),
                    timeout_ms: Some(123),
                    redirect_policy: Some(RedirectPolicy::Follow { max: 7 }),
                }
            );
            assert_eq!(Mock.send(request).await.unwrap_err(), "CORS/fetch failure");
        });
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn malformed_requests_trap_instead_of_reaching_the_provider() {
        futures::executor::block_on(async {
            let mem = Flat(RefCell::new(vec![0; 256]));
            for offset in [12, 32, 48, 56] {
                let mut record = [0; 68];
                record[offset] = 255;
                assert!(decode(&mem, &record).await.is_err(), "tag at {offset}");
            }
            let mut record = [0; 68];
            put(&mut record, 28, u32::MAX);
            assert!(decode(&mem, &record).await.is_err());
            assert!(read(&mem, u32::MAX, 2).await.is_err());
            assert!(read(&mem, 255, 2).await.is_err());
            let mut record = [0; 68];
            record[32] = 1;
            record[36] = 1;
            assert_eq!(
                decode(&mem, &record).await.unwrap().unwrap_err(),
                "File descriptor bodies not supported yet"
            );
        });
    }
}
