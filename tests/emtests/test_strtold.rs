#[test]
#[ignore]
fn test_test_strtold() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtold.wasm",
        "test_strtold",
        vec![],
        "../emscripten_resources/emtests/test_strtold.out"
    );
}
