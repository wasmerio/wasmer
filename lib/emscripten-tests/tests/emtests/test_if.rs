#[test]
fn test_test_if() {
    assert_emscripten_output!(
        "../../emtests/test_if.wasm",
        "test_if",
        vec![],
        "../../emtests/test_if.out"
    );
}
