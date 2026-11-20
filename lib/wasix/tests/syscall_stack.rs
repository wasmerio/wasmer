#![cfg(all(feature = "sys", not(target_family = "wasm")))]
//! Where WASIX syscalls run when Wasm calls them.

use wasmer::{Extern, Store};
use wasmer_wasix::{WasiEnv, WasiVersion, generate_import_object_from_env};

/// Every syscall runs on the native stack, never on the Wasm stack it was
/// called from: syscalls may need more stack than the Wasm stack keeps for
/// host calls.
#[test]
fn syscalls_run_on_the_native_stack() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();

    // A `sys` engine whatever the default backend is: with `v8-default`, as on
    // the Windows test lane, it is V8, which has no Wasm stacks of its own,
    // and `calls_on_native_stack` is always `false` there. Nothing is
    // compiled, so a headless engine is enough to make the imports.
    let engine: wasmer::Engine = wasmer::sys::EngineBuilder::headless().engine().into();
    let mut store = Store::new(engine.clone());
    let env = WasiEnv::builder("syscall-stack")
        .engine(engine)
        .finalize(&mut store)
        .unwrap();
    for version in [
        WasiVersion::Snapshot0,
        WasiVersion::Snapshot1,
        WasiVersion::Wasix32v1,
        WasiVersion::Wasix64v1,
    ] {
        let imports = generate_import_object_from_env(&mut store, &env.env, version);
        let mut syscalls = 0;
        for ((namespace, name), export) in &imports {
            if let Extern::Function(function) = export {
                assert!(
                    function.calls_on_native_stack(&store),
                    "{version:?}: {namespace}.{name}"
                );
                syscalls += 1;
            }
        }
        assert!(syscalls > 40, "{version:?}: only {syscalls} syscalls");
    }
}
