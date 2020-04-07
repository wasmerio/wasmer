#[test]
#[ignore]
fn test_test_sscanf_6() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_6.wasm",
        "test_sscanf_6",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_6.out"
    );
}
