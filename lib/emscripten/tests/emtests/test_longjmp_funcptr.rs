#[test]
fn test_test_longjmp_funcptr() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp_funcptr.wasm",
        "test_longjmp_funcptr",
        vec![],
        "../../emtests/test_longjmp_funcptr.out"
    );
}
