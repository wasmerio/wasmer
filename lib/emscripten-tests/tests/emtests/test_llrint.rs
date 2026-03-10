#[test]
fn test_test_llrint() {
    assert_emscripten_output!(
        "../../emtests/test_llrint.wasm",
        "test_llrint",
        vec![],
        "../../emtests/test_llrint.out"
    );
}
