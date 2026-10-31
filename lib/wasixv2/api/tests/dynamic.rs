//! Bounded native loader proof using externally source-built C fixtures. Guest program
//! sources belong to wasixv2-tests; no compiled wasm is checked into this repository.
#![cfg(all(feature = "sys", feature = "tokio", not(target_arch = "wasm32")))]

use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use wasixv2::{ByteSink, EmptySource, LocalFuture, Machine, Stdio};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);
impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

#[tokio::test]
#[ignore = "requires source-built EH/PIC native-plugin fixtures via WASIXV2_NATIVE_PLUGIN_DIR"]
async fn native_pic_plugin_with_existing_and_future_peers() {
    let directory =
        PathBuf::from(std::env::var_os("WASIXV2_NATIVE_PLUGIN_DIR").expect("fixture directory"));
    let wasm = std::fs::read(directory.join("main.wasm")).expect("source-built main");
    for file in ["plugin.so", "bad.so", "independent.so", "global.so"] {
        assert!(
            directory.join(file).is_file(),
            "missing source-built {file}"
        );
    }
    let local = tokio::task::LocalSet::new();
    let (result, out, err) = local
        .run_until(async {
            let stdout = Arc::new(Capture::default());
            let stderr = Arc::new(Capture::default());
            let mut machine = Machine::new();
            machine.mount_root_lower_host_dir(&directory).await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(60),
                machine.run(
                    &wasm,
                    Stdio {
                        stdin: Arc::new(EmptySource),
                        stdout: stdout.clone(),
                        stderr: stderr.clone(),
                    },
                ),
            )
            .await;
            let out = String::from_utf8_lossy(&stdout.0.lock().unwrap()).into_owned();
            let err = String::from_utf8_lossy(&stderr.0.lock().unwrap()).into_owned();
            // TokioHost's documented executor context includes Drop/retirement work.
            drop(machine);
            (result, out, err)
        })
        .await;
    tokio::time::timeout(Duration::from_secs(10), local)
        .await
        .expect("main executor cleanup drained");
    let result = result.expect("bounded native plugin execution");
    assert_eq!(result.unwrap(), 0, "stdout: {out}\nstderr: {err}");
    assert!(
        out.contains("native plugin PASS:"),
        "stdout: {out}\nstderr: {err}"
    );
}
