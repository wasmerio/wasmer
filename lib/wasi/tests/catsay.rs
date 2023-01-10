#![cfg(feature = "sys")]
#![cfg(target_os = "linux")]
use std::time::Duration;

#[allow(unused_imports)]
use tracing::{debug, info, metadata::LevelFilter};
#[cfg(feature = "sys")]
use tracing_subscriber::fmt::SubscriberBuilder;
use wasmer::{Module, Store};
use wasmer_vfs::{AsyncReadExt, AsyncWriteExt};
use wasmer_wasi::{Pipe, WasiError, WasiState};

#[cfg(feature = "sys")]
mod sys {
    #[tokio::test]
    async fn test_catsay() {
        super::test_catsay().await
    }
}

#[cfg(feature = "js")]
mod js {
    use wasm_bindgen_test::*;
    #[wasm_bindgen_test]
    fn test_catsay() {
        super::test_catsay()
    }
}

// TODO: make it work on JS
#[cfg(feature = "sys")]
async fn test_catsay() {
    info!("Creating engine");
    let engine = wasmer_wasi::build_test_engine(None);

    #[allow(unused_mut)]
    let mut store = Store::new(engine);

    info!("Compiling module");
    let module = Module::new(&store, include_bytes!("catsay.wasm")).unwrap();

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

    let engine = store.engine().clone();
    for _ in 0..10 {
        let module = module.clone();
        run_test(store, module).await;

        store = Store::new(engine.clone());
    }

    // TODO: This version will SIGSEGV (users must reuse engines)
    for _ in 0..10 {
        let module = module.clone();
        run_test(store, module).await;

        store = Store::new(engine.clone());
    }
}

async fn run_test(mut store: Store, module: Module) {
    // Create the `WasiEnv`.
    let mut stdout = Pipe::default();
    let mut wasi_state_builder = WasiState::builder("catsay");

    let mut stdin_pipe = Pipe::default();

    let mut wasi_env = wasi_state_builder
        .stdin(Box::new(stdin_pipe.clone()))
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

    // Write some text to catsay stdin
    stdin_pipe.write_all("hi there".as_bytes()).await.unwrap();
    drop(stdin_pipe);

    // Generate an `ImportObject`.
    let instance = wasmer_wasi::build_wasi_instance(&module, &mut wasi_env, &mut store).unwrap();

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    let ret = start.call(&mut store, &[]);
    if let Err(e) = ret {
        match e.downcast::<WasiError>() {
            Ok(WasiError::Exit(0)) => {}
            Ok(WasiError::Exit(code)) => {
                assert!(
                    false,
                    "The call should have returned Err(WasiError::Exit(0)) but returned {}",
                    code
                );
            }
            Ok(WasiError::UnknownWasiVersion) => {
                assert!(false, "The call should have returned Err(WasiError::Exit(0)) but returned UnknownWasiVersion");
            }
            Err(err) => {
                assert!(false, "The call returned an error {:?}", err);
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
