#[test]
fn test_test_array2b() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_array2b.wasm",
        "test_array2b",
        vec![],
        "../emscripten_resources/emtests/test_array2b.out"
    );
}
