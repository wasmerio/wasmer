mod wasixcc_test_utils;
use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn lifecycle_of_global_in_shared_library() {
    let wasm = run_build_script(file!(), "lifecycle-of-global-in-shared-library").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn lifecycle_of_global_in_dynamic_library() {
    let wasm = run_build_script(file!(), "lifecycle-of-global-in-dynamic-library").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn lifecycle_of_global_in_main_module() {
    let wasm = run_build_script(file!(), "lifecycle-of-global-in-main-module").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn lifecycle_of_tls_global_in_shared_library() {
    let wasm = run_build_script(file!(), "lifecycle-of-tls-global-in-shared-library").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn lifecycle_of_tls_global_in_dynamic_library() {
    let wasm = run_build_script(file!(), "lifecycle-of-tls-global-in-dynamic-library").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn lifecycle_of_tls_global_in_main_module() {
    let wasm = run_build_script(file!(), "lifecycle-of-tls-global-in-main-module").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn lifecycle_of_dlsymed_tls_global_in_dynamic_library() {
    let wasm = run_build_script(
        file!(),
        "lifecycle-of-dlsymed-tls-global-in-dynamic-library",
    )
    .unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
