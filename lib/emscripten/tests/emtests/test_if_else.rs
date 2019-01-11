#[test]
fn test_test_if_else() {
    assert_emscripten_output!(
        "../../emtests/test_if_else.wasm",
        "test_if_else",
        vec![],
        "../../emtests/test_if_else.out"
    );
}
