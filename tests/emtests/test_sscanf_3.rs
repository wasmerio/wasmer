#[test]
#[ignore]
fn test_test_sscanf_3() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_3.wasm",
        "test_sscanf_3",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_3.out"
    );
}
