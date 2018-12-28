#[test]
fn test_test_strtoll_oct() {
    assert_emscripten_output!(
        "../../emtests/test_strtoll_oct.wasm",
        "test_strtoll_oct",
        vec![],
        "../../emtests/test_strtoll_oct.out"
    );
}
