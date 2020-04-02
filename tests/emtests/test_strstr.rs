#[test]
#[ignore]
fn test_test_strstr() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strstr.wasm",
        "test_strstr",
        vec![],
        "../emscripten_resources/emtests/test_strstr.out"
    );
}
