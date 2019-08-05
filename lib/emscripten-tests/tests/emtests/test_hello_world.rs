#[test]
fn test_test_hello_world() {
    assert_emscripten_output!(
        "../../emtests/test_hello_world.wasm",
        "test_hello_world",
        vec![],
        "../../emtests/test_hello_world.out"
    );
}
