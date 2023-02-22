use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::{Future, StreamExt, TryFutureExt};
use http::{Request, Response};
use hyper::Body;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt},
    runtime::Handle,
};
use wasmer::Engine;
use wasmer_vfs::{FileSystem, PassthruFileSystem, RootFileSystemBuilder, TmpFileSystem};
use wasmer_wasi::{http::HttpClientCapabilityV1, Capabilities, Pipe, WasiEnv};

use crate::{
    module_loader::{LoadedModule, ModuleLoader, ModuleLoaderContext},
    Error,
};

/// The shared object that manages the instantiaion of WASI executables and
/// communicating with them via the CGI protocol.
pub(crate) struct Context {
    pub(crate) engine: Engine,
    pub(crate) env: Arc<HashMap<String, String>>,
    pub(crate) args: Arc<[String]>,
    pub(crate) mapped_dirs: Vec<(String, PathBuf)>,
    pub(crate) tokio_handle: Handle,
    pub(crate) loader: Box<dyn ModuleLoader>,
}

impl Context {
    pub(crate) async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, Error> {
        let LoadedModule {
            program,
            module,
            dialect,
        } = self
            .loader
            .load(ModuleLoaderContext::new(&self.engine, &self.tokio_handle))
            .await?;

        let (parts, body) = req.into_parts();

        let (req_body_sender, req_body_receiver) = Pipe::channel();
        let (res_body_sender, res_body_receiver) = Pipe::channel();
        let (stderr_sender, stderr_receiver) = Pipe::channel();

        let builder = WasiEnv::builder(program.to_string());

        let mut request_specific_env = HashMap::new();
        dialect.prepare_environment_variables(parts, &mut request_specific_env);

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
            .sandbox_fs(self.fs()?)
            .preopen_dir(Path::new("/"))?;

        let done = self
            .tokio_handle
            .spawn_blocking(move || builder.run(module))
            .map_err(Error::from)
            .and_then(|r| async { r.map_err(Error::from) });

        let handle = self.tokio_handle.clone();
        self.tokio_handle.spawn(async move {
            if let Err(e) =
                drive_request_to_completion(&handle, done, body, req_body_sender, stderr_receiver)
                    .await
            {
                tracing::error!(
                    error = &e as &dyn std::error::Error,
                    "Unable to drive the request to completion"
                );
            }
        });

        let mut res_body_receiver = tokio::io::BufReader::new(res_body_receiver);

        let parts = dialect
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
                Arc::new(PassthruFileSystem::new(wasmer_wasi::default_fs_backing()));

            for (src, dst) in &self.mapped_dirs {
                let src = match src.starts_with('/') {
                    true => PathBuf::from(src),
                    false => Path::new("/").join(src),
                };
                tracing::trace!(
                    source=%src.display(),
                    alias=%dst.display(),
                    "mounting directory to instance fs",
                );

                root_fs
                    .mount(PathBuf::from(&src), &fs_backing, dst.clone())
                    .map_err(|error| Error::Mount {
                        error,
                        src,
                        dst: dst.to_path_buf(),
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
    instance_stderr: impl AsyncRead + Send + Unpin + 'static,
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
                instance_stdin
                    .write_all(chunk.as_ref())
                    .await
                    .map_err(Error::Io)?;
            }

            instance_stdin.shutdown().await.map_err(Error::Io)?;

            Ok::<(), Error>(())
        })
        .map_err(Error::from)
        .and_then(|r| async { r });

    handle.spawn(async move {
        consume_stderr(instance_stderr).await;
    });

    futures::try_join!(done, request_body_send)?;

    Ok(())
}

/// Read the instance's stderr, taking care to preserve output even when WASI
/// pipe errors occur so users still have *something* they use for
/// troubleshooting.
async fn consume_stderr(stderr: impl AsyncRead + Send + Unpin + 'static) {
    let mut stderr = tokio::io::BufReader::new(stderr);

    // FIXME: this could lead to unbound memory usage
    let mut buffer = Vec::new();

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
                buffer.extend(chunk);
                let bytes_read = chunk.len();
                stderr.consume(bytes_read);
            }
            Err(e) => {
                tracing::error!(
                    error = &e as &dyn std::error::Error,
                    bytes_read = buffer.len(),
                    "Unable to read the complete stderr",
                );
                break;
            }
        }
    }

    let stderr = String::from_utf8(buffer).unwrap_or_else(|e| {
        tracing::warn!(
            error = &e as &dyn std::error::Error,
            "Stdout wasn't valid UTF-8",
        );
        String::from_utf8_lossy(e.as_bytes()).into_owned()
    });

    tracing::info!(%stderr);
}
