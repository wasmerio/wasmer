#[test]
fn test_test_strtoll_dec() {
    assert_emscripten_output!(
        "../../emtests/test_strtoll_dec.wasm",
        "test_strtoll_dec",
        vec![],
        "../../emtests/test_strtoll_dec.out"
    );
}
