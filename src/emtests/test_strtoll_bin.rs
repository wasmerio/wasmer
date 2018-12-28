#[test]
fn test_test_strtoll_bin() {
    assert_emscripten_output!(
        "../../emtests/test_strtoll_bin.wasm",
        "test_strtoll_bin",
        vec![],
        "../../emtests/test_strtoll_bin.out"
    );
}
