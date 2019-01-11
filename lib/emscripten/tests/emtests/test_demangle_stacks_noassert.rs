#[test]
#[ignore]
fn test_test_demangle_stacks_noassert() {
    assert_emscripten_output!(
        "../../emtests/test_demangle_stacks_noassert.wasm",
        "test_demangle_stacks_noassert",
        vec![],
        "../../emtests/test_demangle_stacks_noassert.out"
    );
}
