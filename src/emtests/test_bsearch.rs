#[test]
fn test_test_bsearch() {
    assert_emscripten_output!(
        "../../emtests/test_bsearch.wasm",
        "test_bsearch",
        vec![],
        "../../emtests/test_bsearch.out"
    );
}
