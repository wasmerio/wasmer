#[test]
fn test_test_frexp() {
    assert_emscripten_output!(
        "../../emtests/test_frexp.wasm",
        "test_frexp",
        vec![],
        "../../emtests/test_frexp.out"
    );
}
