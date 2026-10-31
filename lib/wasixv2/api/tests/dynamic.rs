//! Bounded native loader proof using externally source-built C fixtures. Guest program
//! sources belong to wasixv2-tests; no compiled wasm is checked into this repository.
#![cfg(all(feature = "sys", feature = "tokio", not(target_arch = "wasm32")))]

use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use wasixv2::{ByteSink, Credentials, EmptySource, LocalFuture, Machine, ProcessConfig, Stdio};

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
    for file in ["plugin.so", "bad.so", "independent.so", "global.so"] {
        assert!(
            directory.join(file).is_file(),
            "missing source-built {file}"
        );
    }
    run_fixture(directory, "native plugin PASS:").await;
}

#[tokio::test]
#[ignore = "requires source-built EH/PIC cyclic fixtures via WASIXV2_NATIVE_CYCLES_DIR"]
async fn native_pic_cyclic_dependencies_with_existing_and_future_peers() {
    let directory =
        PathBuf::from(std::env::var_os("WASIXV2_NATIVE_CYCLES_DIR").expect("fixture directory"));
    for file in ["main.wasm", "cycle-a.so", "cycle-b.so"] {
        assert!(
            directory.join(file).is_file(),
            "missing source-built {file}"
        );
    }
    run_fixture(directory, "native cycles PASS:").await;
}

async fn run_fixture(directory: PathBuf, expected: &str) {
    run_fixture_image(directory, "main.wasm", expected, Vec::new()).await;
}

#[tokio::test]
#[ignore = "requires source-built startup fixtures via WASIXV2_NATIVE_STARTUP_DIR"]
async fn native_pic_startup_needed_and_preload_with_future_peers() {
    let directory =
        PathBuf::from(std::env::var_os("WASIXV2_NATIVE_STARTUP_DIR").expect("fixture directory"));
    for image in ["main.wasm", "main-preload-only.wasm"] {
        run_fixture_image(
            directory.clone(),
            image,
            if image == "main.wasm" {
                "native startup PASS:"
            } else {
                "native startup preload-only PASS:"
            },
            vec!["LD_PRELOAD=/startup-preload.so".into()],
        )
        .await;
    }
}

async fn run_fixture_image(directory: PathBuf, image: &str, expected: &str, env: Vec<String>) {
    let wasm = std::fs::read(directory.join(image)).expect("source-built main");
    let local = tokio::task::LocalSet::new();
    let (result, out, err) = local
        .run_until(async {
            let stdout = Arc::new(Capture::default());
            let stderr = Arc::new(Capture::default());
            let mut machine = Machine::new();
            machine.mount_root_lower_host_dir(&directory).await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(60),
                machine.run_process(
                    &wasm,
                    ProcessConfig {
                        args: Vec::new(),
                        env,
                        stdio: Stdio {
                            stdin: Arc::new(EmptySource),
                            stdout: stdout.clone(),
                            stderr: stderr.clone(),
                        },
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
    assert!(out.contains(expected), "stdout: {out}\nstderr: {err}");
}

// The tiny exec launcher follows retirement.rs's native exec proof. Startup C
// sources stay external; old/new environments deliberately name different files.
fn startup_exec_launcher(path: &str, preload: &str, secure: bool) -> Vec<u8> {
    let setid = if secure {
        r#"(import "wasixv2" "setresuid" (func $setid (param i32 i32 i32) (result i32)))"#
    } else {
        ""
    };
    let drop_privilege = if secure {
        "i32.const 0 i32.const 999 i32.const 0 call $setid if i32.const 88 call $exit end"
    } else {
        ""
    };
    wat::parse_str(format!(
        r#"(module
        {setid}
        (import "wasixv2" "execve" (func $exec (param i32 i32 i32) (result i32)))
        (import "wasixv2" "exit_group" (func $exit (param i32)))
        (memory (export "memory") 1)
        (data (i32.const 32) "\80\00\00\00\00\00\00\00")
        (data (i32.const 64) "\a0\00\00\00\00\00\00\00")
        (data (i32.const 128) "{path}\00")
        (data (i32.const 160) "LD_PRELOAD={preload}\00")
        (func (export "_start")
            {drop_privilege}
            i32.const 128 i32.const 32 i32.const 64 call $exec drop
            i32.const 37 call $exit))"#
    ))
    .unwrap()
}

#[tokio::test]
#[ignore = "requires source-built startup fixtures via WASIXV2_NATIVE_STARTUP_DIR"]
async fn native_startup_exec_uses_new_environment_and_existing_failure_boundary() {
    let directory =
        PathBuf::from(std::env::var_os("WASIXV2_NATIVE_STARTUP_DIR").expect("fixture directory"));
    for (path, preload, expected, secure) in [
        ("/main.wasm", "/startup-preload.so", 0, false),
        ("/main.wasm", "/missing-new.so", 139, false),
        ("/main.wasm", "/prepare-failure.so", 139, false),
        ("/secure-main.wasm", "/missing-new.so", 0, true),
        ("/missing-image.wasm", "/startup-preload.so", 37, false),
    ] {
        let local = tokio::task::LocalSet::new();
        let (result, out, err) = local
            .run_until(async {
                let stdout = Arc::new(Capture::default());
                let stderr = Arc::new(Capture::default());
                let mut machine = if secure {
                    Machine::builder().user(Credentials::new(0, 0)).build()
                } else {
                    Machine::new()
                };
                machine.mount_root_lower_host_dir(&directory).await.unwrap();
                let result = tokio::time::timeout(
                    Duration::from_secs(60),
                    machine.run_process(
                        &startup_exec_launcher(path, preload, secure),
                        ProcessConfig {
                            args: Vec::new(),
                            env: vec![
                                "LD_PRELOAD=/missing-old.so".into(),
                                "LD_LIBRARY_PATH=/missing-old-dir".into(),
                            ],
                            stdio: Stdio {
                                stdin: Arc::new(EmptySource),
                                stdout: stdout.clone(),
                                stderr: stderr.clone(),
                            },
                        },
                    ),
                )
                .await;
                let out = String::from_utf8_lossy(&stdout.0.lock().unwrap()).into_owned();
                let err = String::from_utf8_lossy(&stderr.0.lock().unwrap()).into_owned();
                drop(machine);
                (result, out, err)
            })
            .await;
        tokio::time::timeout(Duration::from_secs(10), local)
            .await
            .expect("startup exec cleanup");
        assert_eq!(
            result.unwrap().unwrap(),
            expected,
            "{path} / {preload}: {out} / {err}"
        );
        if expected == 0 {
            assert!(
                out.contains(if secure {
                    "native secure startup PASS:"
                } else {
                    "native startup PASS:"
                }),
                "{out} / {err}"
            );
        }
    }
}
