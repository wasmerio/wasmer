#[test]
#[ignore]
fn test_test_execvp() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_execvp.wasm",
        "test_execvp",
        vec![],
        "../emscripten_resources/emtests/test_execvp.out"
    );
}
