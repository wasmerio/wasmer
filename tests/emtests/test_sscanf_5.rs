#[test]
#[ignore]
fn test_test_sscanf_5() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_5.wasm",
        "test_sscanf_5",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_5.out"
    );
}
