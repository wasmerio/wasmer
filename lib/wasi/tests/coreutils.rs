#[cfg(feature = "sys")]
use tracing_subscriber::fmt::SubscriberBuilder;
use wasmer::{Features, Module, Store};
use wasmer_vfs::AsyncReadExt;
use wasmer_wasi::{Pipe, WasiError, WasiState};

#[cfg(feature = "sys")]
mod sys {
    #[tokio::test]
    async fn test_coreutils() {
        super::test_coreutils().await
    }
}

// TODO: run on JS again
// #[cfg(feature = "js")]
// mod js {
//     use wasm_bindgen_test::*;
//     #[wasm_bindgen_test]
//     fn test_coreutils() {
//         super::test_coreutils()
//     }
// }

// TODO: run on JS again
#[cfg(feature = "sys")]
async fn test_coreutils() {
    use tracing::{info, metadata::LevelFilter};

    let mut features = Features::new();
    features.threads(true);

    info!("Creating engine");
    let engine = wasmer_wasi::build_test_engine(Some(features));
    let store = Store::new(engine.clone());

    info!("Compiling module");
    let module = Module::new(&store, include_bytes!("coreutils.wasm")).unwrap();

    #[cfg(feature = "js")]
    tracing_wasm::set_as_global_default_with_config({
        let mut builder = tracing_wasm::WASMLayerConfigBuilder::new();
        builder.set_console_config(tracing_wasm::ConsoleConfig::ReportWithoutConsoleColor);
        builder.build()
    });

    #[cfg(feature = "sys")]
    SubscriberBuilder::default()
        .with_max_level(LevelFilter::DEBUG)
        .init();

    // We do it many times (to make sure the compiled modules are reusable)
    for n in 0..2 {
        let store = Store::new(engine.clone());
        let module = module.clone();

        // Run the test itself
        info!("Test Round {}", n);
        run_test(store, module).await;
    }
}

async fn run_test(mut store: Store, module: Module) {
    // Create the `WasiEnv`.
    let mut stdout = Pipe::default();
    let mut wasi_state_builder = WasiState::builder("echo");
    wasi_state_builder.args(&["apple"]);

    let mut wasi_env = wasi_state_builder
        .stdout(Box::new(stdout.clone()))
        .finalize(&mut store)
        .unwrap();

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

    let mut stdout_str = String::new();
    stdout.read_to_string(&mut stdout_str).await.unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "apple\n");
}
