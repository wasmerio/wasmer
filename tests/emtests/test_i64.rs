#[test]
#[ignore]
fn test_test_i64() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64.wasm",
        "test_i64",
        vec![],
        "../emscripten_resources/emtests/test_i64.out"
    );
}
