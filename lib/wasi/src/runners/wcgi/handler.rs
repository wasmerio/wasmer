use std::{collections::HashMap, ops::Deref, pin::Pin, sync::Arc, task::Poll};

use anyhow::Error;
use futures::{Future, FutureExt, StreamExt, TryFutureExt};
use http::{Request, Response};
use hyper::{service::Service, Body};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt},
    runtime::Handle,
};
use tracing::Instrument;
use wasmer::Module;
use wcgi_host::CgiDialect;

use crate::{
    capabilities::Capabilities, http::HttpClientCapabilityV1, runners::wcgi::Callbacks, Pipe,
    PluggableRuntime, VirtualTaskManager, WasiEnvBuilder,
};

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
#[derive(Clone, Debug)]
pub(crate) struct Handler(Arc<SharedState>);

impl Handler {
    pub(crate) fn new(state: SharedState) -> Self {
        Handler(Arc::new(state))
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub(crate) async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, Error> {
        tracing::debug!(headers=?req.headers());

        let (parts, body) = req.into_parts();

        let (req_body_sender, req_body_receiver) = Pipe::channel();
        let (res_body_sender, res_body_receiver) = Pipe::channel();
        let (stderr_sender, stderr_receiver) = Pipe::channel();

        tracing::debug!("Creating the WebAssembly instance");

        let mut builder = WasiEnvBuilder::new(&self.program_name);

        (self.setup_builder)(&mut builder)?;

        // Note: We want to apply the CGI environment variables *after*
        // anything specified by WASI annotations so users get a chance to
        // override things like $DOCUMENT_ROOT and $SCRIPT_FILENAME.
        let mut request_specific_env = HashMap::new();
        self.dialect
            .prepare_environment_variables(parts, &mut request_specific_env);
        builder.add_envs(request_specific_env);

        let rt = PluggableRuntime::new(Arc::clone(&self.task_manager));

        let builder = builder
            .stdin(Box::new(req_body_receiver))
            .stdout(Box::new(res_body_sender))
            .stderr(Box::new(stderr_sender))
            .capabilities(Capabilities {
                insecure_allow_all: true,
                http_client: HttpClientCapabilityV1::new_allow_all(),
                threading: Default::default(),
            })
            .runtime(Arc::new(rt));

        let module = self.module.clone();

        tracing::debug!(
            dialect=%self.dialect,
            "Calling into the WCGI executable",
        );

        let done = self
            .task_manager
            .runtime()
            .spawn_blocking(move || builder.run(module))
            .map_err(Error::from)
            .and_then(|r| async { r.map_err(Error::from) });

        let handle = self.task_manager.runtime().clone();
        let callbacks = Arc::clone(&self.callbacks);

        handle.spawn(
            async move {
                consume_stderr(stderr_receiver, callbacks).await;
            }
            .in_current_span(),
        );

        self.task_manager.runtime().spawn(
            async move {
                if let Err(e) =
                    drive_request_to_completion(&handle, done, body, req_body_sender).await
                {
                    tracing::error!(
                        error = &*e as &dyn std::error::Error,
                        "Unable to drive the request to completion"
                    );
                }
            }
            .in_current_span(),
        );

        let mut res_body_receiver = tokio::io::BufReader::new(res_body_receiver);

        let parts = self
            .dialect
            .extract_response_header(&mut res_body_receiver)
            .await?;
        let chunks = futures::stream::try_unfold(res_body_receiver, |mut r| async move {
            match r.fill_buf().await {
                Ok(chunk) if chunk.is_empty() => Ok(None),
                Ok(chunk) => {
                    let chunk = chunk.to_vec();
                    r.consume(chunk.len());
                    Ok(Some((chunk, r)))
                }
                Err(e) => Err(e),
            }
        });
        let body = hyper::Body::wrap_stream(chunks);

        let response = hyper::Response::from_parts(parts, body);

        Ok(response)
    }
}

impl Deref for Handler {
    type Target = Arc<SharedState>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Drive the request to completion by streaming the request body to the
/// instance and waiting for it to exit.
async fn drive_request_to_completion(
    handle: &Handle,
    done: impl Future<Output = Result<(), Error>>,
    mut request_body: hyper::Body,
    mut instance_stdin: impl AsyncWrite + Send + Unpin + 'static,
) -> Result<(), Error> {
    let request_body_send = handle
        .spawn(
            async move {
                // Copy the request into our instance, chunk-by-chunk. If the instance
                // dies before we finish writing the body, the instance's side of the
                // pipe will be automatically closed and we'll error out.
                let mut request_size = 0;
                while let Some(res) = request_body.next().await {
                    // FIXME(theduke): figure out how to propagate a body error to the
                    // CGI instance.
                    let chunk = res?;
                    request_size += chunk.len();
                    instance_stdin.write_all(chunk.as_ref()).await?;
                }

                instance_stdin.shutdown().await?;
                tracing::debug!(
                    request_size,
                    "Finished forwarding the request to the WCGI server"
                );

                Ok::<(), Error>(())
            }
            .in_current_span(),
        )
        .map_err(Error::from)
        .and_then(|r| async { r });

    futures::try_join!(done, request_body_send)?;

    Ok(())
}

/// Read the instance's stderr, taking care to preserve output even when WASI
/// pipe errors occur so users still have *something* they use for
/// troubleshooting.
async fn consume_stderr(
    stderr: impl AsyncRead + Send + Unpin + 'static,
    callbacks: Arc<dyn Callbacks>,
) {
    let mut stderr = tokio::io::BufReader::new(stderr);

    // Note: we don't want to just read_to_end() because a reading error
    // would cause us to lose all of stderr. At least this way we'll be
    // able to show users the partial result.
    loop {
        match stderr.fill_buf().await {
            Ok(chunk) if chunk.is_empty() => {
                // EOF - the instance's side of the pipe was closed.
                break;
            }
            Ok(chunk) => {
                callbacks.on_stderr(chunk);
                let bytes_read = chunk.len();
                stderr.consume(bytes_read);
            }
            Err(e) => {
                callbacks.on_stderr_error(e);
                break;
            }
        }
    }
}

type SetupBuilder = Box<dyn Fn(&mut WasiEnvBuilder) -> Result<(), anyhow::Error> + Send + Sync>;

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub(crate) struct SharedState {
    pub(crate) module: Module,
    pub(crate) dialect: CgiDialect,
    pub(crate) program_name: String,
    #[derivative(Debug = "ignore")]
    pub(crate) setup_builder: SetupBuilder,
    #[derivative(Debug = "ignore")]
    pub(crate) callbacks: Arc<dyn Callbacks>,
    #[derivative(Debug = "ignore")]
    pub(crate) task_manager: Arc<dyn VirtualTaskManager>,
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
