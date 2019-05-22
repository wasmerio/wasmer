use wasmer_runtime::{func, imports, instantiate};
use wasmer_runtime_core::vm::Ctx;
use wasmer_wasi::generate_import_object;

static PLUGIN_LOCATION: &'static str = "examples/plugin-for-example.wasm";

fn it_works(_ctx: &mut Ctx) -> i32 {
    println!("Hello from outside WASI");
    5
}

fn main() {
    // Load the plugin data
    let wasm_bytes = std::fs::read(PLUGIN_LOCATION).expect(&format!(
        "Could not read in WASM plugin at {}",
        PLUGIN_LOCATION
    ));

    // WASI imports
    let mut base_imports = generate_import_object(vec![], vec![], vec![], vec![]);
    // env is the default namespace for extern functions
    let custom_imports = imports! {
        "env" => {
            "it_works" => func!(it_works),
        },
    };
    // The WASI imports object contains all required import functions for a WASI module to run.
    // Extend this imports with our custom imports containing "it_works" function so that our custom wasm code may run.
    base_imports.extend(custom_imports);
    let instance =
        instantiate(&wasm_bytes[..], &base_imports).expect("failed to instantiate wasm module");

    // get a reference to the function "plugin_entrypoint" which takes an i32 and returns an i32
    let entry_point = instance.func::<(i32), i32>("plugin_entrypoint").unwrap();
    // call the "entry_point" function in WebAssembly with the number "2" as the i32 argument
    let result = entry_point.call(2).expect("failed to execute plugin");
    println!("result: {}", result);
}
