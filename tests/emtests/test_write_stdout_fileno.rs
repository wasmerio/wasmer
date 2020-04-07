#[test]
#[ignore]
fn test_test_write_stdout_fileno() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_write_stdout_fileno.wasm",
        "test_write_stdout_fileno",
        vec![],
        "../emscripten_resources/emtests/test_write_stdout_fileno.out"
    );
}
