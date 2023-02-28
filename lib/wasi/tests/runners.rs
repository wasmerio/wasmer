#![cfg(feature = "webc_runner")]

use std::path::Path;

use reqwest::Client;
use wasmer_wasi::runners::{Runner, WapmContainer};

#[cfg(feature = "webc_runner_rt_wasi")]
mod wasi {
    use tokio::runtime::Handle;
    use wasmer::Store;
    use wasmer_wasi::{
        runners::wasi::WasiRunner, runtime::task_manager::tokio::TokioTaskManager, WasiRuntimeError,
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
        let err = WasiRunner::new(store)
            .with_task_manager(tasks)
            .run_cmd(&container, "wat2wasm")
            .unwrap_err();

        let runtime_error: &WasiRuntimeError = err.downcast().unwrap();
        let exit_code = runtime_error.as_exit_code().unwrap();
        assert_eq!(exit_code, 1);
    }
}

#[cfg(feature = "webc_runner_rt_wcgi")]
mod wcgi {
    use std::thread::JoinHandle;

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
        let port = 12345;
        let (tx, rx) = futures::channel::oneshot::channel();
        runner
            .config()
            .addr(([127, 0, 0, 1], port).into())
            .task_manager(tasks)
            .abort_channel(rx);
        // Note: the server blocks, so spin it up in a background thread and kill it
        // after we've made our request.
        let _guard = thread_spawn(move || {
            runner.run_cmd(&container, "wcgi").unwrap();
        });

        // The way we test this is by fetching "/" and checking it contains
        // something we expect
        let resp = reqwest::get(format!("http://localhost:{port}/index.html"))
            .await
            .unwrap();
        let body = resp.error_for_status().unwrap().text().await.unwrap();

        assert!(body.contains("asdf"), "{}", body);

        // Make sure we shut the server down afterwards
        drop(tx);
    }

    fn thread_spawn(f: impl FnOnce() + Send + 'static) -> impl Drop {
        struct JoinOnDrop(Option<JoinHandle<()>>);
        impl Drop for JoinOnDrop {
            fn drop(&mut self) {
                if let Err(e) = self.0.take().unwrap().join() {
                    if !std::thread::panicking() {
                        std::panic::resume_unwind(e);
                    }
                }
            }
        }
        let handle = std::thread::spawn(f);

        JoinOnDrop(Some(handle))
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

    let client = Client::new();

    let response = client
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
