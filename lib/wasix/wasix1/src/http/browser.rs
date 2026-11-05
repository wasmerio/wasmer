//! Fetch capability backed by B8's independent I/O worker. No JS object crosses a worker.
use super::{HttpProvider, Request, Response};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU32, Ordering},
    },
    task::{Context, Poll, Waker},
};
use wasixv2_syscalls::LocalFuture;
use wasm_bindgen::prelude::*;

/// Per-provider budgets. The embedder sizes them with its runtime heap/admission limits.
#[derive(Clone, Copy)]
pub struct Limits {
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_in_flight: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_bytes: 1024 * 1024,
            max_response_bytes: 1024 * 1024,
            max_in_flight: 16,
        }
    }
}
struct Budget {
    limits: Limits,
    active: AtomicU32,
}
/// Installing this provider explicitly grants browser Fetch access. Browser origin/CORS,
/// forbidden-header and redirect rules still apply; it supplies no TCP/TLS replacement.
#[derive(Clone)]
pub struct FetchProvider(Arc<Budget>);
impl FetchProvider {
    pub fn new(limits: Limits) -> Result<Self, String> {
        if limits.max_request_bytes == 0
            || limits.max_request_bytes > 1024 * 1024
            || limits.max_response_bytes == 0
            || limits.max_response_bytes > 8 * 1024 * 1024
            || limits.max_in_flight == 0
            || limits.max_in_flight > 128
        {
            return Err("invalid browser HTTP limits".into());
        }
        Ok(Self(Arc::new(Budget {
            limits,
            active: AtomicU32::new(0),
        })))
    }
}
#[derive(Default)]
struct Reply {
    result: Option<Result<Response, String>>,
    waker: Option<Waker>,
    completed: bool,
}
struct Pending {
    reply: Mutex<Reply>,
    max_bytes: usize,
}
static NEXT: AtomicU32 = AtomicU32::new(1);
static REQUESTS: LazyLock<Mutex<HashMap<u32, Weak<Pending>>>> = LazyLock::new(Mutex::default);
#[wasm_bindgen(module = "/src/http/fetch-bridge.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = httpSend)]
    fn send(id: u32, request: &str, max_bytes: u32) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = httpAbort)]
    fn abort(id: u32);
}
/// Called briefly by the I/O worker after its Fetch Promise settles. Payloads are owned
/// copies; cancellation removes the weak entry before late callbacks can reach a waiter.
#[wasm_bindgen]
pub fn browser_http_complete(id: u32, metadata: &str, bytes: &[u8], error: &str) {
    let pending = REQUESTS.lock().unwrap().get(&id).and_then(Weak::upgrade);
    let Some(pending) = pending else {
        return;
    };
    let result = if !error.is_empty() {
        Err(error.chars().take(4096).collect())
    } else if bytes.len() > pending.max_bytes || metadata.len() > 65536 {
        Err("HTTP response exceeded provider budget".into())
    } else {
        serde_json::from_str::<Response>(metadata)
            .map(|mut response| {
                response.body = bytes.to_vec();
                response
            })
            .map_err(|e| e.to_string())
    };
    let waker = {
        let mut reply = pending.reply.lock().unwrap();
        if reply.completed {
            return;
        }
        reply.completed = true;
        reply.result = Some(result);
        reply.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}
struct Fetch {
    id: u32,
    pending: Arc<Pending>,
    budget: Arc<Budget>,
}
impl Future for Fetch {
    type Output = Result<Response, String>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut reply = self.pending.reply.lock().unwrap();
        if let Some(result) = reply.result.take() {
            Poll::Ready(result)
        } else {
            reply.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
impl Drop for Fetch {
    fn drop(&mut self) {
        REQUESTS.lock().unwrap().remove(&self.id);
        if !self.pending.reply.lock().unwrap().completed {
            abort(self.id);
        }
        self.budget.active.fetch_sub(1, Ordering::AcqRel);
    }
}
impl HttpProvider for FetchProvider {
    fn send(&self, request: Request) -> LocalFuture<'_, Result<Response, String>> {
        Box::pin(async move {
            let size = request.headers.iter().try_fold(
                request
                    .url
                    .len()
                    .checked_add(request.method.len())
                    .and_then(|n| n.checked_add(request.body.as_ref().map_or(0, Vec::len)))
                    .unwrap_or(usize::MAX),
                |n, (k, v)| n.checked_add(k.len())?.checked_add(v.len()),
            );
            if size.is_none_or(|n| n > self.0.limits.max_request_bytes) {
                return Err("HTTP request exceeded provider budget".into());
            }
            self.0
                .active
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    (n < self.0.limits.max_in_flight).then_some(n + 1)
                })
                .map_err(|_| "too many HTTP requests")?;
            let id = match NEXT
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            {
                Ok(id) => id,
                Err(_) => {
                    self.0.active.fetch_sub(1, Ordering::AcqRel);
                    return Err("HTTP request IDs exhausted".into());
                }
            };
            let pending = Arc::new(Pending {
                reply: Mutex::default(),
                max_bytes: self.0.limits.max_response_bytes,
            });
            let request_future = Fetch {
                id,
                pending: pending.clone(),
                budget: self.0.clone(),
            };
            REQUESTS
                .lock()
                .unwrap()
                .insert(id, Arc::downgrade(&pending));
            let metadata = serde_json::to_string(&request).map_err(|e| e.to_string())?;
            send(id, &metadata, self.0.limits.max_response_bytes as u32)
                .map_err(|e| format!("HTTP I/O worker unavailable: {e:?}"))?;
            request_future.await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn budgets_fail_before_dispatching_worker_io() {
        for limits in [
            Limits {
                max_in_flight: 0,
                ..Limits::default()
            },
            Limits {
                max_request_bytes: 1024 * 1024 + 1,
                ..Limits::default()
            },
            Limits {
                max_response_bytes: 8 * 1024 * 1024 + 1,
                ..Limits::default()
            },
        ] {
            assert!(FetchProvider::new(limits).is_err());
        }
        let provider = FetchProvider::new(Limits {
            max_request_bytes: 1,
            ..Limits::default()
        })
        .unwrap();
        let result = provider
            .send(Request {
                url: "https://example.test/".into(),
                method: "GET".into(),
                headers: vec![],
                body: None,
                timeout_ms: None,
                redirect_policy: None,
            })
            .await;
        assert_eq!(result.unwrap_err(), "HTTP request exceeded provider budget");
        assert_eq!(provider.0.active.load(Ordering::Acquire), 0);
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    fn completion_wakes_once_and_releases_its_request_budget() {
        struct Wake(AtomicUsize);
        impl std::task::Wake for Wake {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let provider = FetchProvider::new(Limits::default()).unwrap();
        provider.0.active.store(1, Ordering::Release);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let pending = Arc::new(Pending {
            reply: Mutex::default(),
            max_bytes: 8,
        });
        let mut future = Fetch {
            id,
            pending: pending.clone(),
            budget: provider.0.clone(),
        };
        REQUESTS
            .lock()
            .unwrap()
            .insert(id, Arc::downgrade(&pending));
        let wakes = Arc::new(Wake(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
        let metadata = r#"{"status":201,"headers":[],"body":[],"redirect_urls":null}"#;
        browser_http_complete(id, metadata, &[0, 128, 255], "");
        browser_http_complete(id, metadata, &[], "late error");
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
        let Poll::Ready(Ok(response)) = Pin::new(&mut future).poll(&mut cx) else {
            panic!("completion lost");
        };
        assert_eq!(response.status, 201);
        assert_eq!(response.body, [0, 128, 255]);
        drop(future);
        assert!(!REQUESTS.lock().unwrap().contains_key(&id));
        assert_eq!(provider.0.active.load(Ordering::Acquire), 0);
        browser_http_complete(id, metadata, &[], "late after cancellation");
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    }
}
