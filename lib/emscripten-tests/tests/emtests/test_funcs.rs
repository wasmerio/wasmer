#[test]
fn test_test_funcs() {
    assert_emscripten_output!(
        "../../emtests/test_funcs.wasm",
        "test_funcs",
        vec![],
        "../../emtests/test_funcs.out"
    );
}
