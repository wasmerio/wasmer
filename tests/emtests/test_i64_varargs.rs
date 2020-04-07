#[test]
#[ignore]
fn test_test_i64_varargs() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_varargs.wasm",
        "test_i64_varargs",
        vec![],
        "../emscripten_resources/emtests/test_i64_varargs.out"
    );
}
