use std::{ops::Deref, pin::Pin, sync::Arc, task::Poll};

use anyhow::Error;
use futures::Future;
use http::{Request, Response};
use hyper::{service::Service, Body};

use crate::runners::wcgi;

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
#[derive(Clone, Debug)]
pub(crate) struct Handler {
    state: Arc<SharedState>,
    inner: wcgi::Handler,
}

impl Handler {
    pub(crate) fn new(state: SharedState) -> Self {
        Handler {
            inner: wcgi::Handler::new(state.inner.clone()),
            state: Arc::new(state),
        }
    }

    pub(crate) fn from_wcgi_handler(handler: wcgi::Handler) -> Self {
        Handler {
            state: Arc::new(SharedState {
                inner: handler.deref().clone(),
            }),
            inner: handler,
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub(crate) async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, Error> {
        self.inner.handle(req).await
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
}

impl Service<Request<Body>> for Handler {
    type Response = Response<Body>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        self.inner.call(request)
    }
}
