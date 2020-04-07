#[test]
#[ignore]
fn test_test_sscanf_4() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_4.wasm",
        "test_sscanf_4",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_4.out"
    );
}
