#[test]
#[ignore]
fn test_test_pipe() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_pipe.wasm",
        "test_pipe",
        vec![],
        "../emscripten_resources/emtests/test_pipe.out"
    );
}
