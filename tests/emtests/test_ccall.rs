#[test]
#[ignore]
fn test_test_ccall() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_ccall.wasm",
        "test_ccall",
        vec![],
        "../emscripten_resources/emtests/test_ccall.out"
    );
}
