#[test]
#[ignore]
fn test_test_varargs_multi() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_varargs_multi.wasm",
        "test_varargs_multi",
        vec![],
        "../emscripten_resources/emtests/test_varargs_multi.out"
    );
}
