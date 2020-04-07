#[test]
#[ignore]
fn test_test_strtoll_dec() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtoll_dec.wasm",
        "test_strtoll_dec",
        vec![],
        "../emscripten_resources/emtests/test_strtoll_dec.out"
    );
}
