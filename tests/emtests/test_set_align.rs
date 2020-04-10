#[test]
#[ignore]
fn test_test_set_align() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_set_align.wasm",
        "test_set_align",
        vec![],
        "../emscripten_resources/emtests/test_set_align.out"
    );
}
