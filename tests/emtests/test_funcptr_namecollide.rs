#[test]
fn test_test_funcptr_namecollide() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_funcptr_namecollide.wasm",
        "test_funcptr_namecollide",
        vec![],
        "../emscripten_resources/emtests/test_funcptr_namecollide.out"
    );
}
