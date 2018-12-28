#[test]
fn test_test_nl_types() {
    assert_emscripten_output!(
        "../../emtests/test_nl_types.wasm",
        "test_nl_types",
        vec![],
        "../../emtests/test_nl_types.out"
    );
}
