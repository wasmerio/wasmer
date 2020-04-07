#[test]
#[ignore]
fn test_test_sscanf() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf.wasm",
        "test_sscanf",
        vec![],
        "../emscripten_resources/emtests/test_sscanf.out"
    );
}
