#[test]
fn test_test_indirectbr() {
    assert_emscripten_output!(
        "../../emtests/test_indirectbr.wasm",
        "test_indirectbr",
        vec![],
        "../../emtests/test_indirectbr.out"
    );
}
