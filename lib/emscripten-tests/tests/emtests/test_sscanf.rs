#[test]
#[ignore]
fn test_test_sscanf() {
    assert_emscripten_output!(
        "../../emtests/test_sscanf.wasm",
        "test_sscanf",
        vec![],
        "../../emtests/test_sscanf.out"
    );
}
