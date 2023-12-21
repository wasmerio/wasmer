use std::{collections::HashMap, ops::Deref, pin::Pin, sync::Arc, task::Poll};

use anyhow::Error;
use futures::{Future, FutureExt, StreamExt};
use http::{Request, Response, StatusCode};
use hyper::{service::Service, Body};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};
use tracing::Instrument;
use wasmer::Module;
use wcgi_host::CgiDialect;

use crate::{
    runners::wcgi::{
        callbacks::{CreateEnvConfig, RecycleEnvConfig},
        Callbacks,
    },
    runtime::module_cache::ModuleHash,
    Runtime, VirtualTaskManager, WasiEnvBuilder,
};

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
#[derive(Clone, Debug)]
pub(crate) struct Handler<M>(Arc<SharedState<M>>)
where
    M: Send + Sync + 'static;

impl<M> Handler<M>
where
    M: Send + Sync + 'static,
{
    pub(crate) fn new(state: Arc<SharedState<M>>) -> Self {
        Handler(state)
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub(crate) async fn handle(&self, req: Request<Body>, meta: M) -> Result<Response<Body>, Error>
    where
        M: Clone,
    {
        tracing::debug!(headers=?req.headers());

        let (parts, body) = req.into_parts();

        // Note: We want to apply the CGI environment variables *after*
        // anything specified by WASI annotations so users get a chance to
        // override things like $DOCUMENT_ROOT and $SCRIPT_FILENAME.
        let mut request_specific_env = HashMap::new();
        request_specific_env.insert("REQUEST_METHOD".to_string(), parts.method.to_string());
        request_specific_env.insert("SCRIPT_NAME".to_string(), parts.uri.path().to_string());
        if let Some(query) = parts.uri.query() {
            request_specific_env.insert("QUERY_STRING".to_string(), query.to_string());
        }
        self.dialect
            .prepare_environment_variables(parts, &mut request_specific_env);

        let create = self
            .callbacks
            .create_env(CreateEnvConfig {
                meta: meta.clone(),
                env: request_specific_env,
                program_name: self.program_name.clone(),
                module: self.module.clone(),
                module_hash: self.module_hash,
                runtime: self.runtime.clone(),
                setup_builder: self.setup_builder.clone(),
            })
            .await?;

        tracing::debug!(
            dialect=%self.dialect,
            "Calling into the WCGI executable",
        );

        let task_manager = self.runtime.task_manager();
        let env = create.env;
        let store = create.store;
        let (run_tx, mut run_rx) = tokio::sync::mpsc::unbounded_channel();
        task_manager.task_dedicated(Box::new(move || {
            run_tx.send(env.run_async(store)).ok();
        }))?;
        let done = async move { run_rx.recv().await.unwrap().map_err(Error::from) };

        let mut res_body_receiver = tokio::io::BufReader::new(create.body_receiver);

        let stderr_receiver = create.stderr_receiver;
        let callbacks = Arc::clone(&self.callbacks);
        let propagate_stderr = self.propagate_stderr;
        let work_consume_stderr = {
            let callbacks = callbacks.clone();
            async move { consume_stderr(stderr_receiver, callbacks, propagate_stderr).await }
                .in_current_span()
        };

        tracing::trace!(
            dialect=%self.dialect,
            "spawning request forwarder",
        );

        let req_body_sender = create.body_sender;
        let callbacks = Arc::clone(&self.callbacks);
        let work_drive_io = {
            async move {
                let ret = drive_request_to_completion(done, body, req_body_sender).await;
                match ret {
                    Ok((env, store)) => {
                        callbacks
                            .recycle_env(RecycleEnvConfig { meta, env, store })
                            .await;
                    }
                    Err(e) => {
                        tracing::error!(
                            error = &*e as &dyn std::error::Error,
                            "Unable to drive the request to completion"
                        );
                    }
                }
            }
            .in_current_span()
        };
        task_manager
            .task_shared(Box::new(move || Box::pin(work_drive_io)))
            .ok();

        tracing::trace!(
            dialect=%self.dialect,
            "extracting response parts",
        );

        let parts = self
            .dialect
            .extract_response_header(&mut res_body_receiver)
            .await;

        // When set this will cause any stderr responses to
        // take precedence over nominal responses but it
        // will cause the stderr pipe to be read to the end
        // before transmitting the body
        if propagate_stderr {
            if let Some(stderr) = work_consume_stderr.await {
                if !stderr.is_empty() {
                    return Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from(stderr))?);
                }
            }
        } else {
            task_manager
                .task_shared(Box::new(move || {
                    Box::pin(async move {
                        work_consume_stderr.await;
                    })
                }))
                .ok();
        }
        let parts = parts?;

        tracing::trace!(
            dialect=%self.dialect,
            status=%parts.status,
            "received response parts",
        );

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

        tracing::trace!(
            dialect=%self.dialect,
            "returning response with body stream",
        );

        let response = hyper::Response::from_parts(parts, body);
        Ok(response)
    }
}

impl<M> Deref for Handler<M>
where
    M: Send + Sync + 'static,
{
    type Target = Arc<SharedState<M>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Drive the request to completion by streaming the request body to the
/// instance and waiting for it to exit.
async fn drive_request_to_completion<R>(
    done: impl Future<Output = Result<R, Error>>,
    mut request_body: hyper::Body,
    mut instance_stdin: impl AsyncWrite + Send + Unpin + 'static,
) -> Result<R, Error> {
    let request_body_send = async move {
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
    .in_current_span();

    let (ret, _) = futures::try_join!(done, request_body_send)?;
    Ok(ret)
}

/// Read the instance's stderr, taking care to preserve output even when WASI
/// pipe errors occur so users still have *something* they use for
/// troubleshooting.
async fn consume_stderr<M>(
    stderr: impl AsyncRead + Send + Unpin + 'static,
    callbacks: Arc<dyn Callbacks<M>>,
    propagate_stderr: bool,
) -> Option<Vec<u8>>
where
    M: Send + Sync + 'static,
{
    let mut stderr = tokio::io::BufReader::new(stderr);

    let mut propagate = match propagate_stderr {
        true => Some(Vec::new()),
        false => None,
    };

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
                tracing::trace!("received stderr (len={})", chunk.len());
                if let Some(propogate) = propagate.as_mut() {
                    propogate.write_all(chunk).await.ok();
                }
                callbacks.on_stderr(chunk);
                let bytes_read = chunk.len();
                stderr.consume(bytes_read);
            }
            Err(e) => {
                tracing::trace!("received stderr (err={})", e);
                callbacks.on_stderr_error(e);
                break;
            }
        }
    }

    propagate
}

pub type SetupBuilder = Arc<dyn Fn(&mut WasiEnvBuilder) -> Result<(), anyhow::Error> + Send + Sync>;

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub(crate) struct SharedState<M>
where
    M: Send + Sync + 'static,
{
    pub(crate) module: Module,
    pub(crate) module_hash: ModuleHash,
    pub(crate) dialect: CgiDialect,
    pub(crate) program_name: String,
    pub(crate) propagate_stderr: bool,
    #[derivative(Debug = "ignore")]
    pub(crate) setup_builder: SetupBuilder,
    #[derivative(Debug = "ignore")]
    pub(crate) callbacks: Arc<dyn Callbacks<M>>,
    #[derivative(Debug = "ignore")]
    pub(crate) runtime: Arc<dyn Runtime + Send + Sync>,
}

impl Service<Request<Body>> for Handler<()> {
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
        let fut = async move { handler.handle(request, ()).await };
        fut.boxed()
    }
}
