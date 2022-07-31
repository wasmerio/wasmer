use std::io::Read;

#[allow(unused_imports)]
use tracing::{debug, info, metadata::LevelFilter};
#[cfg(feature = "sys")]
use tracing_subscriber::fmt::SubscriberBuilder;
use wasmer::{Instance, Module, Store, Features, Cranelift, EngineBuilder};
use wasmer_wasi::{Pipe, WasiState, import_object_for_all_wasi_versions, WasiError};

mod sys {
    #[test]
    fn test_coreutils() {
        super::test_coreutils()
    }
}

#[cfg(feature = "js")]
mod js {
    use wasm_bindgen_test::*;
    #[wasm_bindgen_test]
    fn test_coreutils() {
        super::test_coreutils()
    }
}

fn test_coreutils() {
    let mut features = Features::new();
    features
        .threads(true);

    info!("Creating engine");
    let compiler = Cranelift::default();
    let engine = EngineBuilder::new(compiler)
        .set_features(Some(features));

    let store = Store::new(engine);

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

    // Package the store so it can be recreated many times
    let packaged_store = store.package();

    // We do it many times (to make sure the compiled modules are reusable)
    for n in 0..2
    {
        let store = packaged_store.clone().unpack();
        let module = module.clone();

        // Run the test itself
        info!("Test Round {}", n);
        run_test(store, module);
    }
}

fn run_test(mut store: Store, module: Module)
{
    // Create the `WasiEnv`.
    let mut stdout = Pipe::new();
    let mut wasi_state_builder = WasiState::new("echo");
    wasi_state_builder
        .args(&["apple"]);
    
    let mut wasi_env = wasi_state_builder
        .stdout(Box::new(stdout.clone()))
        .finalize(&mut store)
        .unwrap();

    // Generate an `ImportObject`.
    let mut import_object = import_object_for_all_wasi_versions(&mut store, &wasi_env.env);
    import_object.import_shared_memory(&module, &mut store);

    // Let's instantiate the module with the imports.
    let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    wasi_env.initialize(&mut store, &instance).unwrap();

    // Let's call the `_start` function, which is our `main` function in Rust.
    let start = instance.exports.get_function("_start").unwrap();
    let ret = start.call(&mut store, &[]);
    if let Err(e) = ret {
        match e.downcast::<WasiError>() {
            Ok(WasiError::Exit(0)) => { }
            _ => {
                assert!(false, "The call should have returned Err(WasiError::Exit(0))");        
            }
        }
    } else {
        assert!(false, "The call should have returned Err(WasiError::Exit(0))");
    }

    let mut stdout_str = String::new();
    stdout.read_to_string(&mut stdout_str).unwrap();
    let stdout_as_str = stdout_str.as_str();
    assert_eq!(stdout_as_str, "apple\n");
}
