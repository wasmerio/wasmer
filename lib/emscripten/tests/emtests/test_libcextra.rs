#[test]
fn test_test_libcextra() {
    assert_emscripten_output!(
        "../../emtests/test_libcextra.wasm",
        "test_libcextra",
        vec![],
        "../../emtests/test_libcextra.out"
    );
}
