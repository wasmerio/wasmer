use std::{ops::Deref, pin::Pin, sync::Arc, task::Poll};

use anyhow::Error;
use futures::{Future, FutureExt};
use http::{Request, Response};
use hyper::{service::Service, Body};

use crate::runners::wcgi;

use super::DcgiInstanceFactory;

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
#[derive(Clone, Debug)]
pub(crate) struct Handler {
    state: Arc<SharedState>,
    inner: wcgi::Handler,
}

impl Handler {
    pub(crate) fn new(handler: wcgi::Handler) -> Self {
        Handler {
            state: Arc::new(SharedState {
                inner: handler.deref().clone(),
                factory: DcgiInstanceFactory::new(),
                master_lock: Default::default(),
            }),
            inner: handler,
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub(crate) async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, Error> {
        // we acquire a guard token so that only one request at a time can be processed
        // which effectively means that DCGI is single-threaded. This is a limitation
        // of the MVP which should be rectified in future releases.
        let guard_token = self.state.master_lock.clone().lock_owned().await;

        // Process the request as a normal WCGI request
        self.inner.handle(req, guard_token).await
    }
}

impl Deref for Handler {
    type Target = Arc<SharedState>;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

#[derive(derivative::Derivative, Clone)]
#[derivative(Debug)]
pub(crate) struct SharedState {
    pub(crate) inner: Arc<wcgi::SharedState>,
    factory: DcgiInstanceFactory,
    master_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Service<Request<Body>> for Handler {
    type Response = Response<Body>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        // TODO: We probably should implement some sort of backpressure here...
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        // Note: all fields are reference-counted so cloning is pretty cheap
        let handler = self.clone();
        let fut = async move { handler.handle(request).await };
        fut.boxed()
    }
}
