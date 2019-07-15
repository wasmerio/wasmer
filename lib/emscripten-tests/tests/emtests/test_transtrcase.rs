#[test]
#[ignore]
fn test_test_transtrcase() {
    assert_emscripten_output!(
        "../../emtests/test_transtrcase.wasm",
        "test_transtrcase",
        vec![],
        "../../emtests/test_transtrcase.out"
    );
}
