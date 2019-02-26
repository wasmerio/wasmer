#[test]
fn test_clock_gettime() {
    assert_emscripten_output!(
        "../../emtests/test_execvp.wasm",
        "test_execvp",
        vec![],
        "../../emtests/test_execvp.out"
    );
}
