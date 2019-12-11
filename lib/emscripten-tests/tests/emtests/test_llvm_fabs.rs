#[test]
fn test_test_llvm_fabs() {
    assert_emscripten_output!(
        "../../emtests/test_llvm_fabs.wasm",
        "test_llvm_fabs",
        vec![],
        "../../emtests/test_llvm_fabs.out"
    );
}
