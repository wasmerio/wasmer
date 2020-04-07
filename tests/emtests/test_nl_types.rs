#[test]
#[ignore]
fn test_test_nl_types() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_nl_types.wasm",
        "test_nl_types",
        vec![],
        "../emscripten_resources/emtests/test_nl_types.out"
    );
}
