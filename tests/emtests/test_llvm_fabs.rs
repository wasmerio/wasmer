#[test]
fn test_test_llvm_fabs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_llvm_fabs.wasm",
        "test_llvm_fabs",
        vec![],
        "../emscripten_resources/emtests/test_llvm_fabs.out"
    );
}
