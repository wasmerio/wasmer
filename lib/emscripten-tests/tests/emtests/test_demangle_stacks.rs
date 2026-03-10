#[test]
#[ignore]
fn test_test_demangle_stacks() {
    assert_emscripten_output!(
        "../../emtests/test_demangle_stacks.wasm",
        "test_demangle_stacks",
        vec![],
        "../../emtests/test_demangle_stacks.out"
    );
}
