#[test]
#[ignore]
fn test_test_llvm_intrinsics() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_llvm_intrinsics.wasm",
        "test_llvm_intrinsics",
        vec![],
        "../emscripten_resources/emtests/test_llvm_intrinsics.out"
    );
}
