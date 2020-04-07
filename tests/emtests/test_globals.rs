#[test]
fn test_test_globals() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_globals.wasm",
        "test_globals",
        vec![],
        "../emscripten_resources/emtests/test_globals.out"
    );
}
