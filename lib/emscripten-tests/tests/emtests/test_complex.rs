#[test]
fn test_test_complex() {
    assert_emscripten_output!(
        "../../emtests/test_complex.wasm",
        "test_complex",
        vec![],
        "../../emtests/test_complex.out"
    );
}
