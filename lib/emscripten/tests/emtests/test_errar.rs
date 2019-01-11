#[test]
fn test_test_errar() {
    assert_emscripten_output!(
        "../../emtests/test_errar.wasm",
        "test_errar",
        vec![],
        "../../emtests/test_errar.out"
    );
}
