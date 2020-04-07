#[test]
fn test_test_llvmswitch() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_llvmswitch.wasm",
        "test_llvmswitch",
        vec![],
        "../emscripten_resources/emtests/test_llvmswitch.out"
    );
}
