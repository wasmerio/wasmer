#[test]
fn test_test_longjmp_funcptr() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp_funcptr.wasm",
        "test_longjmp_funcptr",
        vec![],
        "../emscripten_resources/emtests/test_longjmp_funcptr.out"
    );
}
