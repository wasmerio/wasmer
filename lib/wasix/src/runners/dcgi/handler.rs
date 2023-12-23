use std::{ops::Deref, pin::Pin, sync::Arc, task::Poll};

use anyhow::Error;
use futures::{Future, FutureExt};
use http::{Request, Response};
use hyper::{service::Service, Body};

use crate::runners::wcgi;

use super::{DcgiInstanceFactory, DcgiMetadata};

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
#[derive(Clone, Debug)]
pub(crate) struct Handler {
    state: Arc<SharedState>,
    inner: wcgi::Handler<DcgiMetadata>,
}

impl Handler {
    pub(crate) fn from_wcgi_handler(handler: wcgi::Handler<DcgiMetadata>) -> Self {
        Handler {
            state: Arc::new(SharedState {
                inner: handler.deref().clone(),
                factory: DcgiInstanceFactory::default(),
            }),
            inner: handler,
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub(crate) async fn handle(
        &self,
        req: Request<Body>,
        meta: DcgiMetadata,
    ) -> Result<Response<Body>, Error> {
        self.inner.handle(req, meta).await
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
    pub(crate) inner: Arc<wcgi::SharedState<DcgiMetadata>>,
    factory: DcgiInstanceFactory,
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
        // We determine the shard that this DCGI request will run against
        // (multiple shards can be served by the same endpoint)
        let shard = request
            .headers()
            .get("X-Shard")
            .map(|s| s.to_str().unwrap_or("").to_string())
            .unwrap_or_else(|| "".to_string());

        // Grab the metadata from the request
        let meta = DcgiMetadata {
            shard,
            master_lock: None,
        };

        // Note: all fields are reference-counted so cloning is pretty cheap
        let handler = self.clone();
        let fut = async move { handler.handle(request, meta).await };
        fut.boxed()
    }
}
