#[test]
fn test_pipe() {
    assert_emscripten_output!(
        "../../emtests/test_pipe.wasm",
        "test_pipe",
        vec![],
        "../../emtests/test_pipe.out"
    );
}
