use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use anyhow::Error;
use futures::{Future, FutureExt, StreamExt, TryFutureExt};
use http::{Request, Response};
use hyper::{service::Service, Body};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt},
    runtime::Handle,
};
use wasmer::Module;
use wasmer_vfs::{FileSystem, PassthruFileSystem, RootFileSystemBuilder, TmpFileSystem};
use wcgi_host::CgiDialect;

use crate::{
    http::HttpClientCapabilityV1,
    runners::wcgi::{Callbacks, MappedDirectory},
    Capabilities, Pipe, PluggableRuntimeImplementation, VirtualTaskManager, WasiEnv,
};

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub(crate) struct Handler {
    pub(crate) program: Arc<str>,
    pub(crate) env: Arc<HashMap<String, String>>,
    pub(crate) args: Arc<[String]>,
    pub(crate) mapped_dirs: Arc<[MappedDirectory]>,
    pub(crate) task_manager: Arc<dyn VirtualTaskManager>,
    pub(crate) module: Module,
    pub(crate) dialect: CgiDialect,
    #[derivative(Debug = "ignore")]
    pub(crate) callbacks: Arc<dyn Callbacks>,
}

impl Handler {
    pub(crate) async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, Error> {
        let (parts, body) = req.into_parts();

        let (req_body_sender, req_body_receiver) = Pipe::channel();
        let (res_body_sender, res_body_receiver) = Pipe::channel();
        let (stderr_sender, stderr_receiver) = Pipe::channel();

        let builder = WasiEnv::builder(self.program.to_string());

        let mut request_specific_env = HashMap::new();
        self.dialect
            .prepare_environment_variables(parts, &mut request_specific_env);

        let rt = PluggableRuntimeImplementation::new(Arc::clone(&self.task_manager));
        let builder = builder
            .envs(self.env.iter())
            .envs(request_specific_env)
            .args(self.args.iter())
            .stdin(Box::new(req_body_receiver))
            .stdout(Box::new(res_body_sender))
            .stderr(Box::new(stderr_sender))
            .capabilities(Capabilities {
                insecure_allow_all: true,
                http_client: HttpClientCapabilityV1::new_allow_all(),
            })
            .runtime(Arc::new(rt))
            .sandbox_fs(self.fs()?)
            .preopen_dir(Path::new("/"))?;

        let module = self.module.clone();

        let done = self
            .task_manager
            .runtime()
            .spawn_blocking(move || builder.run(module))
            .map_err(Error::from)
            .and_then(|r| async { r.map_err(Error::from) });

        let handle = self.task_manager.runtime().clone();
        let callbacks = Arc::clone(&self.callbacks);

        handle.spawn(async move {
            consume_stderr(stderr_receiver, callbacks).await;
        });

        self.task_manager.runtime().spawn(async move {
            if let Err(e) = drive_request_to_completion(&handle, done, body, req_body_sender).await
            {
                tracing::error!(
                    error = &*e as &dyn std::error::Error,
                    "Unable to drive the request to completion"
                );
            }
        });

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

    fn fs(&self) -> Result<TmpFileSystem, Error> {
        let root_fs = RootFileSystemBuilder::new().build();

        if !self.mapped_dirs.is_empty() {
            let fs_backing: Arc<dyn FileSystem + Send + Sync> =
                Arc::new(PassthruFileSystem::new(crate::default_fs_backing()));

            for MappedDirectory { host, guest } in self.mapped_dirs.iter() {
                let guest = match guest.starts_with('/') {
                    true => PathBuf::from(guest),
                    false => Path::new("/").join(guest),
                };
                tracing::trace!(
                    host=%host.display(),
                    guest=%guest.display(),
                    "mounting directory to instance fs",
                );

                root_fs
                    .mount(host.clone(), &fs_backing, guest.clone())
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "Unable to mount \"{}\" to \"{}\": {error}",
                            host.display(),
                            guest.display()
                        )
                    })?;
            }
        }
        Ok(root_fs)
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
        .spawn(async move {
            // Copy the request into our instance, chunk-by-chunk. If the instance
            // dies before we finish writing the body, the instance's side of the
            // pipe will be automatically closed and we'll error out.
            while let Some(res) = request_body.next().await {
                // FIXME(theduke): figure out how to propagate a body error to the
                // CGI instance.
                let chunk = res?;
                instance_stdin.write_all(chunk.as_ref()).await?;
            }

            instance_stdin.shutdown().await?;

            Ok::<(), Error>(())
        })
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
