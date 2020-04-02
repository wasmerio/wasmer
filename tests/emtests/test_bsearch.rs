#[test]
fn test_test_bsearch() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_bsearch.wasm",
        "test_bsearch",
        vec![],
        "../emscripten_resources/emtests/test_bsearch.out"
    );
}
