#[test]
fn test_test_globaldoubles() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_globaldoubles.wasm",
        "test_globaldoubles",
        vec![],
        "../emscripten_resources/emtests/test_globaldoubles.out"
    );
}
