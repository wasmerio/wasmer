#[test]
#[ignore]
fn test_test_varargs() {
    assert_emscripten_output!(
        "../../emtests/test_varargs.wasm",
        "test_varargs",
        vec![],
        "../../emtests/test_varargs.out"
    );
}
