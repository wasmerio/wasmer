#[test]
fn test_test_memmove2() {
    assert_emscripten_output!(
        "../../emtests/test_memmove2.wasm",
        "test_memmove2",
        vec![],
        "../../emtests/test_memmove2.out"
    );
}
