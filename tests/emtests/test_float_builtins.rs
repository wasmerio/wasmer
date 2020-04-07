#[test]
#[ignore]
fn test_test_float_builtins() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_float_builtins.wasm",
        "test_float_builtins",
        vec![],
        "../emscripten_resources/emtests/test_float_builtins.out"
    );
}
