#[test]
fn test_test_memcpy2() {
    assert_emscripten_output!(
        "../../emtests/test_memcpy2.wasm",
        "test_memcpy2",
        vec![],
        "../../emtests/test_memcpy2.out"
    );
}
