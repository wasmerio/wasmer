#![cfg(feature = "webc_runner")]

use std::{path::Path, time::Duration};

use once_cell::sync::Lazy;
use reqwest::Client;
use wasmer_wasi::runners::{Runner, WapmContainer};

#[cfg(feature = "webc_runner_rt_wasi")]
mod wasi {
    use tokio::runtime::Handle;
    use wasmer::Store;
    use wasmer_wasi::{
        runners::wasi::WasiRunner, runtime::task_manager::tokio::TokioTaskManager, WasiError,
    };

    use super::*;

    #[tokio::test]
    async fn wat_2_wasm() {
        let webc = download_cached("https://wapm.io/wasmer/wabt").await;
        let store = Store::default();
        let tasks = TokioTaskManager::new(Handle::current());
        let container = WapmContainer::from_bytes(webc).unwrap();

        // Note: we don't have any way to intercept stdin or stdout, so blindly
        // assume that everything is fine if it runs successfully.
        let handle = std::thread::spawn(move || {
            WasiRunner::new(store)
                .with_task_manager(tasks)
                .run_cmd(&container, "wat2wasm")
        });
        let err = handle.join().unwrap().unwrap_err();

        let runtime_error = err
            .chain()
            .find_map(|e| e.downcast_ref::<WasiError>())
            .unwrap();
        let exit_code = match runtime_error {
            WasiError::Exit(code) => *code,
            _ => unreachable!(),
        };
        assert_eq!(exit_code, 1);
    }
}

#[cfg(feature = "webc_runner_rt_wcgi")]
mod wcgi {
    use std::future::Future;

    use futures::{channel::mpsc::Sender, future::AbortHandle, SinkExt, StreamExt};
    use rand::Rng;
    use tokio::runtime::Handle;
    use wasmer_wasi::{runners::wcgi::WcgiRunner, runtime::task_manager::tokio::TokioTaskManager};

    use super::*;

    #[tokio::test]
    async fn static_server() {
        let webc = download_cached("https://wapm.dev/syrusakbary/staticserver").await;
        let tasks = TokioTaskManager::new(Handle::current());
        let container = WapmContainer::from_bytes(webc).unwrap();
        let mut runner = WcgiRunner::new("staticserver");
        let port = rand::thread_rng().gen_range(10000_u16..65535_u16);
        let (cb, started) = callbacks(Handle::current());
        runner
            .config()
            .addr(([127, 0, 0, 1], port).into())
            .task_manager(tasks)
            .callbacks(cb);

        // The server blocks so we need to start it on a background thread.
        std::thread::spawn(move || {
            runner.run_cmd(&container, "wcgi").unwrap();
        });

        // wait for the server to have started
        let abort_handle = started.await;

        // Now the server is running, we can check that it is working by
        // fetching "/" and checking for known content
        let resp = client()
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .unwrap();
        let body = resp.error_for_status().unwrap().text().await.unwrap();

        assert!(body.contains("<title>Index of /</title>"), "{}", body);

        // Make sure the server is shutdown afterwards
        abort_handle.abort();
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

    impl wasmer_wasi::runners::wcgi::Callbacks for Callbacks {
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
    let cache_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(module_path!());
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
    static CLIENT: Lazy<Client> = Lazy::new(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .build()
            .unwrap()
    });
    CLIENT.clone()
}
