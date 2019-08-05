#[test]
fn test_test_funcptr() {
    assert_emscripten_output!(
        "../../emtests/test_funcptr.wasm",
        "test_funcptr",
        vec![],
        "../../emtests/test_funcptr.out"
    );
}
