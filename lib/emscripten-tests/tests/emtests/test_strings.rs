#[test]
#[ignore]
fn test_test_strings() {
    assert_emscripten_output!(
        "../../emtests/test_strings.wasm",
        "test_strings",
        vec![],
        "../../emtests/test_strings.out"
    );
}
