#[test]
#[ignore]
fn test_test_memcpy_memcmp() {
    assert_emscripten_output!(
        "../../emtests/test_memcpy_memcmp.wasm",
        "test_memcpy_memcmp",
        vec![],
        "../../emtests/test_memcpy_memcmp.out"
    );
}
