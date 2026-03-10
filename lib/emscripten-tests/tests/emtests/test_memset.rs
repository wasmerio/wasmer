#[test]
fn test_test_memset() {
    assert_emscripten_output!(
        "../../emtests/test_memset.wasm",
        "test_memset",
        vec![],
        "../../emtests/test_memset.out"
    );
}
