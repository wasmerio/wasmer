#[test]
#[ignore]
fn test_test_sscanf_3() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_3.wasm",
        "test_sscanf_3",
        vec![],
        "../../emtests/test_sscanf_3.out"
    );
}
