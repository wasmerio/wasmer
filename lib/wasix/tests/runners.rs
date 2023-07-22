// Exclude runner tests from wasm targets for now, since they don't run properly
// there.
#![cfg(not(target_family = "wasm"))]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use reqwest::Client;
use tokio::runtime::Handle;
use wasmer::Engine;
use wasmer_wasix::{
    runners::Runner,
    runtime::{
        module_cache::{FileSystemCache, ModuleCache, SharedCache},
        package_loader::BuiltinPackageLoader,
        task_manager::tokio::TokioTaskManager,
    },
    PluggableRuntime, Runtime,
};
use webc::Container;

mod wasi {
    use wasmer_wasix::{bin_factory::BinaryPackage, runners::wasi::WasiRunner, WasiError};

    use super::*;

    #[tokio::test]
    async fn can_run_wat2wasm() {
        let webc = download_cached("https://wasmer.io/wasmer/wabt").await;
        let container = Container::from_bytes(webc).unwrap();
        let command = &container.manifest().commands["wat2wasm"];

        assert!(WasiRunner::can_run_command(command).unwrap());
    }

    #[tokio::test]
    async fn wat2wasm() {
        let webc = download_cached("https://wasmer.io/wasmer/wabt").await;
        let container = Container::from_bytes(webc).unwrap();
        let rt = runtime();
        let pkg = BinaryPackage::from_webc(&container, &rt).await.unwrap();

        // Note: we don't have any way to intercept stdin or stdout, so blindly
        // assume that everything is fine if it runs successfully.
        let handle = std::thread::spawn(move || {
            WasiRunner::new()
                .with_args(["--version"])
                .run_command("wat2wasm", &pkg, Arc::new(rt))
        });

        handle.join().unwrap().expect("Runner failed");
    }

    #[tokio::test]
    async fn python() {
        let webc = download_cached("https://wasmer.io/python/python").await;
        let rt = runtime();
        let container = Container::from_bytes(webc).unwrap();
        let pkg = BinaryPackage::from_webc(&container, &rt).await.unwrap();

        let handle = std::thread::spawn(move || {
            WasiRunner::new()
                .with_args(["-c", "import sys; sys.exit(42)"])
                .run_command("python", &pkg, Arc::new(rt))
        });
        let err = handle.join().unwrap().unwrap_err();

        let runtime_error = err.chain().find_map(|e| e.downcast_ref::<WasiError>());
        let exit_code = match runtime_error {
            Some(WasiError::Exit(code)) => *code,
            Some(other) => panic!("Something else went wrong: {:?}", other),
            None => panic!("Not a WasiError: {:?}", err),
        };
        assert_eq!(exit_code.raw(), 42);
    }
}

#[cfg(feature = "webc_runner_rt_wcgi")]
mod wcgi {
    use std::{future::Future, sync::Arc};

    use futures::{channel::mpsc::Sender, future::AbortHandle, SinkExt, StreamExt};
    use rand::Rng;
    use tokio::runtime::Handle;
    use wasmer_wasix::{bin_factory::BinaryPackage, runners::wcgi::WcgiRunner};

    use super::*;

    #[tokio::test]
    async fn can_run_staticserver() {
        let webc = download_cached("https://wasmer.io/Michael-F-Bryan/staticserver").await;
        let container = Container::from_bytes(webc).unwrap();

        let entrypoint = container.manifest().entrypoint.as_ref().unwrap();
        assert!(WcgiRunner::can_run_command(&container.manifest().commands[entrypoint]).unwrap());
    }

    #[tokio::test]
    async fn staticserver() {
        let webc = download_cached("https://wasmer.io/Michael-F-Bryan/staticserver").await;
        let rt = runtime();
        let container = Container::from_bytes(webc).unwrap();
        let mut runner = WcgiRunner::new();
        let port = rand::thread_rng().gen_range(10000_u16..65535_u16);
        let (cb, started) = callbacks(Handle::current());
        runner
            .config()
            .addr(([127, 0, 0, 1], port).into())
            .callbacks(cb);
        let pkg = BinaryPackage::from_webc(&container, &rt).await.unwrap();

        // The server blocks so we need to start it on a background thread.
        let join_handle = std::thread::spawn(move || {
            runner.run_command("serve", &pkg, Arc::new(rt)).unwrap();
        });

        // wait for the server to have started
        let abort_handle = started.await;

        // Now the server is running, we can check that it is working by
        // fetching "/" and checking for known content. We also want the server
        // to close connections immediately so the graceful shutdown can kill
        // the server immediately instead of waiting for the connection to time
        // out.
        let resp = client()
            .get(format!("http://localhost:{port}/"))
            .header("Connection", "close")
            .send()
            .await
            .unwrap();
        let body = resp.error_for_status().unwrap().text().await.unwrap();
        assert!(body.contains("<title>Index of /</title>"), "{}", body);

        // Make sure the server is shutdown afterwards. Failing tests will leak
        // the server thread, but that's fine.
        abort_handle.abort();
        if let Err(e) = join_handle.join() {
            std::panic::resume_unwind(e);
        }
    }

    fn callbacks(handle: Handle) -> (Callbacks, impl Future<Output = AbortHandle>) {
        let (sender, mut rx) = futures::channel::mpsc::channel(1);

        let cb = Callbacks { sender, handle };
        let fut = async move { rx.next().await.unwrap() };

        (cb, fut)
    }

    struct Callbacks {
        sender: Sender<AbortHandle>,
        handle: Handle,
    }

    impl wasmer_wasix::runners::wcgi::Callbacks for Callbacks {
        fn started(&self, abort: futures::stream::AbortHandle) {
            let mut sender = self.sender.clone();
            self.handle.spawn(async move {
                sender.send(abort).await.unwrap();
            });
        }

        fn on_stderr(&self, stderr: &[u8]) {
            panic!(
                "Something was written to stderr: {}",
                String::from_utf8_lossy(stderr)
            );
        }
    }
}

async fn download_cached(url: &str) -> bytes::Bytes {
    let uri: http::Uri = url.parse().unwrap();

    let file_name = Path::new(uri.path()).file_name().unwrap();
    let cache_dir = tmp_dir().join("downloads");
    let cached_path = cache_dir.join(file_name);

    if cached_path.exists() {
        return std::fs::read(&cached_path).unwrap().into();
    }

    let response = client()
        .get(url)
        .header("Accept", "application/webc")
        .send()
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        200,
        "Unable to get \"{url}\": {}",
        response.status(),
    );

    let body = response.bytes().await.unwrap();

    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(&cached_path, &body).unwrap();

    body
}

fn client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .unwrap()
}

#[cfg(not(target_os = "windows"))]
fn sanitze_name_for_path(name: &str) -> String {
    name.into()
}
#[cfg(target_os = "windows")]
fn sanitze_name_for_path(name: &str) -> String {
    name.replace(":", "_")
}

fn runtime() -> impl Runtime + Send + Sync {
    let tasks = TokioTaskManager::new(Handle::current());
    let mut rt = PluggableRuntime::new(Arc::new(tasks));

    let cache =
        SharedCache::default().with_fallback(FileSystemCache::new(tmp_dir().join("compiled")));

    let cache_dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(env!("CARGO_PKG_NAME"))
        .join(sanitze_name_for_path(
            std::thread::current().name().unwrap_or("cache"),
        ));

    std::fs::create_dir_all(&cache_dir).unwrap();

    rt.set_engine(Some(Engine::default()))
        .set_module_cache(cache)
        .set_package_loader(BuiltinPackageLoader::new(cache_dir))
        .set_http_client(wasmer_wasix::http::default_http_client().unwrap());

    rt
}

fn tmp_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(env!("CARGO_PKG_NAME"))
        .join(module_path!())
}
