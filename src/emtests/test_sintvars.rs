#[test]
fn test_test_sintvars() {
    assert_emscripten_output!(
        "../../emtests/test_sintvars.wasm",
        "test_sintvars",
        vec![],
        "../../emtests/test_sintvars.out"
    );
}
