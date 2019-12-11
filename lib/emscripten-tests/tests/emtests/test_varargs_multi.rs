#[test]
#[ignore]
fn test_test_varargs_multi() {
    assert_emscripten_output!(
        "../../emtests/test_varargs_multi.wasm",
        "test_varargs_multi",
        vec![],
        "../../emtests/test_varargs_multi.out"
    );
}
