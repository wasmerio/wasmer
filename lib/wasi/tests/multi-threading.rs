#![cfg(feature = "sys")]
#![cfg(target_os = "linux")]
use std::time::Duration;

#[allow(unused_imports)]
use tracing::{debug, info, metadata::LevelFilter};
#[cfg(feature = "sys")]
use tracing_subscriber::fmt::SubscriberBuilder;
use wasmer::{Features, Module, Store};
use wasmer_vfs::AsyncReadExt;
use wasmer_wasi::{Pipe, WasiError, WasiState};

// TODO: make it work on JS.
#[cfg(feature = "sys")]
mod sys {
    #[tokio::test]
    async fn test_multithreading() {
        super::test_multithreading().await
    }
}

// TODO: make it work on JS.
#[cfg(feature = "sys")]
async fn test_multithreading() {
    let mut features = Features::new();
    features.threads(true);

    info!("Creating engine");
    let engine = wasmer_wasi::build_test_engine(Some(features));

    let store = Store::new(engine.clone());

    info!("Compiling module");
    let module = Module::new(&store, include_bytes!("multi-threading.wasm")).unwrap();

    #[cfg(feature = "js")]
    tracing_wasm::set_as_global_default_with_config({
        let mut builder = tracing_wasm::WASMLayerConfigBuilder::new();
        builder.set_console_config(tracing_wasm::ConsoleConfig::ReportWithoutConsoleColor);
        builder.build()
    });

    #[cfg(feature = "sys")]
    SubscriberBuilder::default()
        .with_max_level(LevelFilter::TRACE)
        .init();

    // We do it many times (to make sure the compiled modules are reusable)
    for n in 0..2 {
        let store = Store::new(engine.clone());
        let module = module.clone();

        info!("Test Round {}", n);
        run_test(store, module).await;
    }
}

async fn run_test(mut store: Store, module: Module) {
    // Create the `WasiEnv`.
    let mut stdout = Pipe::default();
    let mut wasi_state_builder = WasiState::builder("multi-threading");

    let mut wasi_env = wasi_state_builder
        .stdout(Box::new(stdout.clone()))
        .stderr(Box::new(stdout.clone()))
        .finalize(&mut store)
        .unwrap();

    // Start a thread that will dump STDOUT to info
    #[cfg(feature = "sys")]
    tokio::task::spawn(async move {
        loop {
            let mut buf = [0u8; 8192];
            if let Ok(amt) = stdout.read(&mut buf[..]).await {
                if amt > 0 {
                    let msg = String::from_utf8_lossy(&buf[0..amt]);
                    for line in msg.lines() {
                        info!("{}", line);
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
            } else {
                break;
            }
        }
    });

    let instance = wasmer_wasi::build_wasi_instance(&module, &mut wasi_env, &mut store).unwrap();

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    let ret = start.call(&mut store, &[]);
    if let Err(e) = ret {
        match e.downcast::<WasiError>() {
            Ok(WasiError::Exit(0)) => {}
            _ => {
                assert!(
                    false,
                    "The call should have returned Err(WasiError::Exit(0))"
                );
            }
        }
    }

    #[cfg(feature = "js")]
    {
        let mut stdout_str = String::new();
        stdout.read_to_string(&mut stdout_str).unwrap();
        let stdout_as_str = stdout_str.as_str();
        for line in stdout_str.lines() {
            info!("{}", line);
        }
    }
}
