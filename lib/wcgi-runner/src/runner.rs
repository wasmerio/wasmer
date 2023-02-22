use std::{future::Future, pin::Pin, sync::Arc, task::Poll};

use futures::FutureExt;
use http::{Request, Response};
use hyper::Body;
use tower_service::Service;

use crate::{context::Context, Builder, Error};

/// A runner for WCGI binaries.
///
/// # Examples
///
/// [`Runner`] implements the [`Service`] trait and is cheaply cloneable
/// so it can be easily integrated with a Hyper server and the Tower ecosystem.
///
/// ```rust,no_run
/// # use std::net::SocketAddr;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let webc = b"...";
/// let address: SocketAddr = ([127, 0, 0, 1], 3000).into();
/// let runner = Runner::builder().build_webc(webc)?;
///
/// let make_service = hyper::service::make_service_fn(move |_| {
///     let runner = runner.clone();
///     async move { Ok::<_, std::convert::Infallible>(runner) }
/// });
///
/// hyper::Server::bind(&address).serve(make_service).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Runner {
    ctx: Arc<Context>,
}

impl Runner {
    pub(crate) fn new(ctx: Arc<Context>) -> Self {
        Runner { ctx }
    }

    /// Create a [`Builder`] that can be used to configure a [`Runner`].
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Handle a single HTTP request.
    pub async fn handle(&self, request: Request<Body>) -> Result<Response<Body>, Error> {
        self.ctx.handle(request).await
    }
}

impl Service<Request<Body>> for Runner {
    type Response = Response<Body>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        // TODO: We probably should implement some sort of backpressure here...
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let ctx = Arc::clone(&self.ctx);
        let fut = async move { ctx.handle(request).await };
        fut.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::extra_unused_type_parameters)]
    fn send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<Runner>();
        assert_sync::<Runner>();
    }
}
