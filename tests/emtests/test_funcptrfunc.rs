#[test]
fn test_test_funcptrfunc() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_funcptrfunc.wasm",
        "test_funcptrfunc",
        vec![],
        "../emscripten_resources/emtests/test_funcptrfunc.out"
    );
}
