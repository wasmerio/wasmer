#![cfg(all(unix, not(target_os = "macos"), not(feature = "js")))]
mod wasixcc_test_utils;

use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn minimal_threadlocal() {
    let wasm = run_build_script(file!(), "minimal-threadlocal").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn minimal_threadlocals() {
    let wasm = run_build_script(file!(), "minimal-threadlocals").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn thread_getspecific_in_main_thread() {
    let wasm = run_build_script(file!(), "thread-getspecific-in-main-thread").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn threadlocal_errno() {
    let wasm = run_build_script(file!(), "threadlocal-errno").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn threadlocal_defined_in_main_used_in_shared_lib() {
    let wasm = run_build_script(file!(), "threadlocal-defined-in-main-used-in-shared-lib").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn threadlocal_in_shared_lib() {
    let wasm = run_build_script(file!(), "threadlocal-in-shared-lib").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn threadlocals_are_actually_threadlocal() {
    let wasm = run_build_script(file!(), "threadlocals-are-actually-threadlocal").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn threadlocals_work_in_a_shared_lib() {
    let wasm = run_build_script(file!(), "threadlocals-work-in-a-shared-lib").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn threadlocals_work_in_a_shared_lib_weird() {
    let wasm = run_build_script(file!(), "threadlocals-work-in-a-shared-lib-weird").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn extern_threadlocal() {
    let wasm = run_build_script(file!(), "extern-threadlocal").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn extern_threadlocal_nopic() {
    let wasm = run_build_script(file!(), "extern-threadlocal-nopic").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

// Grid tests for thread-getspecific set/get with different linking strategies
#[test]
fn tsd_set_direct_in_direct_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_direct_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_direct_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_direct_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_shared_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_shared_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_shared_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_dynamic_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_dynamic_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_direct_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DIRECT-in-DIRECT-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_dynamic_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_dynamic_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_shared_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DIRECT-in-SHARED-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_dynamic_in_direct() {
    let wasm =
        run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_dynamic_in_shared() {
    let wasm =
        run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_direct_in_dynamic_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DIRECT-in-DYNAMIC-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_direct_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_direct_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_direct_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_shared_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_shared_in_dynamic() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_shared_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_dynamic_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_dynamic_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_direct_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-SHARED-in-DIRECT-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_dynamic_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_dynamic_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_shared_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-SHARED-in-SHARED-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_dynamic_in_direct() {
    let wasm =
        run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_dynamic_in_shared() {
    let wasm =
        run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_shared_in_dynamic_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-SHARED-in-DYNAMIC-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_direct_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_direct_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_direct_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_direct_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_direct_in_direct() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-DIRECT-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_direct_in_shared() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-DIRECT-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_direct_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-DIRECT-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_shared_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_shared_in_direct() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_shared_in_shared() {
    let wasm = run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_shared_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_shared_in_direct() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-SHARED-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_shared_in_shared() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-SHARED-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_shared_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-SHARED-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_dynamic_in_direct() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_dynamic_in_shared() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_direct_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DIRECT-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_dynamic_in_direct() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_dynamic_in_shared() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_shared_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-SHARED-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_dynamic_in_direct() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-DYNAMIC-in-DIRECT").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_dynamic_in_shared() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-DYNAMIC-in-SHARED").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}

#[test]
fn tsd_set_dynamic_in_dynamic_get_dynamic_in_dynamic() {
    let wasm =
        run_build_script(file!(), "tsd-set-DYNAMIC-in-DYNAMIC-get-DYNAMIC-in-DYNAMIC").unwrap();
    run_wasm(&wasm, wasm.parent().unwrap()).unwrap();
}
