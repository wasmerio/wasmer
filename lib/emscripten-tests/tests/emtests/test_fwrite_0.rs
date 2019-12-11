#[test]
fn test_test_fwrite_0() {
    assert_emscripten_output!(
        "../../emtests/test_fwrite_0.wasm",
        "test_fwrite_0",
        vec![],
        "../../emtests/test_fwrite_0.out"
    );
}
