#[test]
fn test_test_memcpy3() {
    assert_emscripten_output!(
        "../../emtests/test_memcpy3.wasm",
        "test_memcpy3",
        vec![],
        "../../emtests/test_memcpy3.out"
    );
}
