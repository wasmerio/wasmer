#[test]
#[ignore]
fn test_test_strtoll_bin() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtoll_bin.wasm",
        "test_strtoll_bin",
        vec![],
        "../emscripten_resources/emtests/test_strtoll_bin.out"
    );
}
