#[test]
fn test_test_array2() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_array2.wasm",
        "test_array2",
        vec![],
        "../emscripten_resources/emtests/test_array2.out"
    );
}
