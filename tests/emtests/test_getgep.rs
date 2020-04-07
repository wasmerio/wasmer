#[test]
fn test_test_getgep() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_getgep.wasm",
        "test_getgep",
        vec![],
        "../emscripten_resources/emtests/test_getgep.out"
    );
}
