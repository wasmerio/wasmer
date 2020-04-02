#[test]
#[ignore]
fn test_test_sscanf_n() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sscanf_n.wasm",
        "test_sscanf_n",
        vec![],
        "../emscripten_resources/emtests/test_sscanf_n.out"
    );
}
