#[test]
fn test_test_hello_world() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_hello_world.wasm",
        "test_hello_world",
        vec![],
        "../emscripten_resources/emtests/test_hello_world.out"
    );
}
