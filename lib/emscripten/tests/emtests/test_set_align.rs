#[test]
fn test_test_set_align() {
    assert_emscripten_output!(
        "../../emtests/test_set_align.wasm",
        "test_set_align",
        vec![],
        "../../emtests/test_set_align.out"
    );
}
