use std::{collections::HashMap, ops::Deref, pin::Pin, sync::Arc, task::Poll};

use anyhow::Error;
use futures::{Future, FutureExt, StreamExt};
use http::{Request, Response, StatusCode};
use hyper::{service::Service, Body};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};
use tracing::Instrument;
use virtual_mio::InlineWaker;
use wasmer::Module;
use wasmer_wasix_types::wasi::ExitCode;
use wcgi_host::CgiDialect;

use crate::{
    bin_factory::run_exec,
    os::task::OwnedTaskStatus,
    runners::wcgi::{
        callbacks::{CreateEnvConfig, RecycleEnvConfig},
        Callbacks,
    },
    runtime::{
        module_cache::ModuleHash,
        task_manager::{TaskWasm, TaskWasmRecycleProperties},
    },
    Runtime, VirtualTaskManager, WasiEnvBuilder,
};

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
#[derive(Clone, Debug)]
pub(crate) struct Handler(Arc<SharedState>);

impl Handler {
    pub(crate) fn new(state: Arc<SharedState>) -> Self {
        Handler(state)
    }

    #[tracing::instrument(level = "debug", skip_all, err)]
    pub(crate) async fn handle<T>(
        &self,
        req: Request<Body>,
        token: T,
    ) -> Result<Response<Body>, Error>
    where
        T: Send + 'static,
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
        let module = self.module.clone();

        // The recycle function will attempt to reuse the instance
        let callbacks = Arc::clone(&self.callbacks);
        let recycle = {
            let callbacks = callbacks.clone();
            move |props: TaskWasmRecycleProperties| {
                InlineWaker::block_on(callbacks.recycle_env(RecycleEnvConfig {
                    env: props.env,
                    store: props.store,
                    memory: props.memory,
                }));

                // We release the token after we recycle the environment
                // so that race conditions (such as reusing instances) are
                // avoided
                drop(token);
            }
        };
        let finished = env.process.finished.clone();

        /*
         * TODO: Reusing memory for DCGI calls and not just the file system
         *
         * DCGI does not support reusing the memory for the following reasons
         * 1. The environment variables can not be overridden after libc does its lazy loading
         * 2. The HTTP request variables are passed as environment variables and hence can not be changed
         *    after the first call is made on the memory
         * 3. The `SpawnMemoryType` is not send however this handler is running as a Send async. In order
         *    to fix this the entire handler would need to run in its own non-Send thread.

        // Determine if we are going to create memory and import it or just rely on self creation of memory
        let spawn_type = create
            .memory
            .map(|memory| SpawnMemoryType::ShareMemory(memory, store.as_store_ref()));
        */

        // We run the WCGI thread on the dedicated WASM
        // thread pool that has support for asynchronous
        // threading, etc...
        task_manager
            .task_wasm(
                TaskWasm::new(Box::new(run_exec), env, module, false)
                    //.with_optional_memory(spawn_type)
                    .with_recycle(Box::new(recycle)),
            )
            .map_err(|err| {
                tracing::warn!("failed to execute WCGI thread - {}", err);
                err
            })?;

        let mut res_body_receiver = tokio::io::BufReader::new(create.body_receiver);

        let stderr_receiver = create.stderr_receiver;
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
        let ret = drive_request_to_completion(finished, body, req_body_sender).await;

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

        match ret {
            Ok(_) => {}
            Err(e) => {
                let e = e.to_string();
                tracing::error!(error = e, "Unable to drive the request to completion");
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(e.as_bytes().to_vec()))?);
            }
        }

        tracing::trace!(
            dialect=%self.dialect,
            "extracting response parts",
        );

        let parts = self
            .dialect
            .extract_response_header(&mut res_body_receiver)
            .await;
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

impl Deref for Handler {
    type Target = Arc<SharedState>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Drive the request to completion by streaming the request body to the
/// instance and waiting for it to exit.
async fn drive_request_to_completion(
    finished: Arc<OwnedTaskStatus>,
    mut request_body: hyper::Body,
    mut instance_stdin: impl AsyncWrite + Send + Sync + Unpin + 'static,
) -> Result<ExitCode, Error> {
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

    let (ret, _) = futures::try_join!(finished.await_termination_anyhow(), request_body_send)?;
    Ok(ret)
}

/// Read the instance's stderr, taking care to preserve output even when WASI
/// pipe errors occur so users still have *something* they use for
/// troubleshooting.
async fn consume_stderr(
    stderr: impl AsyncRead + Send + Unpin + 'static,
    callbacks: Arc<dyn Callbacks>,
    propagate_stderr: bool,
) -> Option<Vec<u8>> {
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
pub(crate) struct SharedState {
    pub(crate) module: Module,
    pub(crate) module_hash: ModuleHash,
    pub(crate) dialect: CgiDialect,
    pub(crate) program_name: String,
    pub(crate) propagate_stderr: bool,
    #[derivative(Debug = "ignore")]
    pub(crate) setup_builder: SetupBuilder,
    #[derivative(Debug = "ignore")]
    pub(crate) callbacks: Arc<dyn Callbacks>,
    #[derivative(Debug = "ignore")]
    pub(crate) runtime: Arc<dyn Runtime + Send + Sync>,
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
        let fut = async move { handler.handle(request, ()).await };
        fut.boxed()
    }
}
