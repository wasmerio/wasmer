#[test]
fn test_test_phiundef() {
    assert_emscripten_output!(
        "../../emtests/test_phiundef.wasm",
        "test_phiundef",
        vec![],
        "../../emtests/test_phiundef.out"
    );
}
