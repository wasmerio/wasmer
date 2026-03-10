#[test]
fn test_test_llvmswitch() {
    assert_emscripten_output!(
        "../../emtests/test_llvmswitch.wasm",
        "test_llvmswitch",
        vec![],
        "../../emtests/test_llvmswitch.out"
    );
}
