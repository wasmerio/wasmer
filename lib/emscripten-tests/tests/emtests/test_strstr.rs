#[test]
#[ignore]
fn test_test_strstr() {
    assert_emscripten_output!(
        "../../emtests/test_strstr.wasm",
        "test_strstr",
        vec![],
        "../../emtests/test_strstr.out"
    );
}
