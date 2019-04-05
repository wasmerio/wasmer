#[test]
#[ignore]
fn test_test_sscanf_n() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf_n.wasm",
        "test_sscanf_n",
        vec![],
        "../../emtests/test_sscanf_n.out"
    );
}
