//! Basic tests from wasix-tests directory
//!
//! These tests verify fundamental functionality:
//! - helloworld: Basic printf and return 0

mod wasixcc_test_utils;
use wasixcc_test_utils::{run_build_script, run_wasm};

#[test]
fn test_helloworld() {
    let wasm_path = run_build_script(file!(), "helloworld").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    run_wasm(&wasm_path, test_dir).unwrap();
}
