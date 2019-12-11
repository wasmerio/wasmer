#[test]
#[ignore]
fn test_test_llvm_intrinsics() {
    assert_emscripten_output!(
        "../../emtests/test_llvm_intrinsics.wasm",
        "test_llvm_intrinsics",
        vec![],
        "../../emtests/test_llvm_intrinsics.out"
    );
}
