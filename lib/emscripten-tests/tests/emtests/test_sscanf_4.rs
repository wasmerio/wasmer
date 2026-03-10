#[test]
#[ignore]
fn test_test_sscanf_4() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_4.wasm",
        "test_sscanf_4",
        vec![],
        "../../emtests/test_sscanf_4.out"
    );
}
