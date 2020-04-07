#[test]
#[ignore]
fn test_test_strtoll_oct() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtoll_oct.wasm",
        "test_strtoll_oct",
        vec![],
        "../emscripten_resources/emtests/test_strtoll_oct.out"
    );
}
