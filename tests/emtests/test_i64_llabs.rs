#[test]
fn test_test_i64_llabs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_llabs.wasm",
        "test_i64_llabs",
        vec![],
        "../emscripten_resources/emtests/test_i64_llabs.out"
    );
}
