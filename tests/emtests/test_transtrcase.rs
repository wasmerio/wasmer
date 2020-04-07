#[test]
#[ignore]
fn test_test_transtrcase() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_transtrcase.wasm",
        "test_transtrcase",
        vec![],
        "../emscripten_resources/emtests/test_transtrcase.out"
    );
}
