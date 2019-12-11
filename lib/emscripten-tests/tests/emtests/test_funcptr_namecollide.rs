#[test]
fn test_test_funcptr_namecollide() {
    assert_emscripten_output!(
        "../../emtests/test_funcptr_namecollide.wasm",
        "test_funcptr_namecollide",
        vec![],
        "../../emtests/test_funcptr_namecollide.out"
    );
}
