#[test]
#[ignore]
fn test_test_sscanf_6() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_6.wasm",
        "test_sscanf_6",
        vec![],
        "../../emtests/test_sscanf_6.out"
    );
}
