use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use futures::{Future, FutureExt};
use http::{Request, Response, Uri};
use hyper::Body;
use tower::Service;

use crate::runners::dproxy::shard::Shard;
use crate::Runtime;

use super::factory::DProxyInstanceFactory;
use super::Config;

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct SharedState {
    pub(crate) config: Config,
    pub(crate) command_name: String,
    #[derivative(Debug = "ignore")]
    pub(crate) runtime: Arc<dyn Runtime + Send + Sync>,
    pub(crate) factory: DProxyInstanceFactory,
}

/// Handler which will process DProxy requests
#[derive(Clone, Debug)]
pub struct Handler(Arc<SharedState>);

impl Handler {
    pub(crate) fn new(
        config: Config,
        command_name: String,
        factory: DProxyInstanceFactory,
        runtime: Arc<dyn Runtime + Send + Sync>,
    ) -> Self {
        Handler(Arc::new(SharedState {
            config,
            command_name,
            runtime,
            factory,
        }))
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub(crate) async fn handle<T>(
        &self,
        mut req: Request<Body>,
        _token: T,
    ) -> anyhow::Result<Response<Body>>
    where
        T: Send + 'static,
    {
        tracing::debug!(headers=?req.headers());

        // Determine the shard we are using
        let shard = req
            .headers()
            .get("X-Shard")
            .map(|v| String::from_utf8_lossy(v.as_bytes()))
            .map(|s| match s.parse::<u64>() {
                Ok(id) => Ok(Shard::ById(id)),
                Err(err) => Err(err),
            })
            .unwrap_or(Ok(Shard::Singleton))?;

        // Modify the request URI so that it will work with the hyper proxy
        let mut new_uri = Uri::builder()
            .scheme("http")
            .authority(
                req.uri()
                    .authority()
                    .cloned()
                    .unwrap_or_else(|| "localhost".parse().unwrap()),
            )
            .path_and_query(
                req.uri()
                    .path_and_query()
                    .cloned()
                    .unwrap_or_else(|| "/".parse().unwrap()),
            )
            .build()
            .unwrap();
        std::mem::swap(req.uri_mut(), &mut new_uri);

        // Acquire a DProxy instance
        tracing::debug!("Acquiring DProxy instance instance");
        let instance = self.factory.acquire(self, shard).await?;

        tracing::debug!("Calling into the DProxy instance");
        let client = instance.client.clone();

        // Perform the request
        Ok(client.request(req).await?)
    }
}

impl std::ops::Deref for Handler {
    type Target = Arc<SharedState>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Service<Request<Body>> for Handler {
    type Response = Response<Body>;
    type Error = anyhow::Error;
    type Future = Pin<Box<dyn Future<Output = anyhow::Result<Response<Body>>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        // Note: all fields are reference-counted so cloning is pretty cheap
        let handler = self.clone();
        let fut = async move { handler.handle(request, ()).await };
        fut.boxed()
    }
}
