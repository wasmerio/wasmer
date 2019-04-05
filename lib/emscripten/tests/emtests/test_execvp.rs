#[test]
#[ignore]
fn test_test_execvp() {
    assert_emscripten_output!(
        "../../emtests/test_execvp.wasm",
        "test_execvp",
        vec![],
        "../../emtests/test_execvp.out"
    );
}
