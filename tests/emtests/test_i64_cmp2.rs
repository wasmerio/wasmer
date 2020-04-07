#[test]
fn test_test_i64_cmp2() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_cmp2.wasm",
        "test_i64_cmp2",
        vec![],
        "../emscripten_resources/emtests/test_i64_cmp2.out"
    );
}
