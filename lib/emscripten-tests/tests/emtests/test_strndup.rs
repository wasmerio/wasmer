#[test]
#[ignore]
fn test_test_strndup() {
    assert_emscripten_output!(
        "../../emtests/test_strndup.wasm",
        "test_strndup",
        vec![],
        "../../emtests/test_strndup.out"
    );
}
