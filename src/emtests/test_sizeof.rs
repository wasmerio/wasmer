#[test]
fn test_test_sizeof() {
    assert_emscripten_output!(
        "../../emtests/test_sizeof.wasm",
        "test_sizeof",
        vec![],
        "../../emtests/test_sizeof.out"
    );
}
