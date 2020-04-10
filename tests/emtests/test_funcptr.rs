#[test]
fn test_test_funcptr() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_funcptr.wasm",
        "test_funcptr",
        vec![],
        "../emscripten_resources/emtests/test_funcptr.out"
    );
}
