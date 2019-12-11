#[test]
fn test_test_funcptrfunc() {
    assert_emscripten_output!(
        "../../emtests/test_funcptrfunc.wasm",
        "test_funcptrfunc",
        vec![],
        "../../emtests/test_funcptrfunc.out"
    );
}
