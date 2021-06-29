use std::error;
use wasmer::{Instance, Module, Store};
use wasmer_wasi::WasiState;
use wasmer_wasi_experimental_network::runtime_impl::get_namespace;

fn main() -> Result<(), Box<dyn error::Error>> {
    let store = Store::default();
    let module = Module::from_file(
        &store,
        "../../../target/wasm32-wasi/release/examples/polling_tcp_server_raw.wasm",
    )?;

    let mut wasi_env = WasiState::new("tcp-server").finalize()?;
    let mut import_object = wasi_env.import_object(&module)?;

    let (module_name, namespace) = get_namespace(&store, &wasi_env);
    import_object.register(module_name, namespace);

    let instance = Instance::new(&module, &import_object)?;
    let _results = instance.exports.get_function("_start")?.call(&[])?;

    Ok(())
}
