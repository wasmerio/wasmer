#[test]
#[ignore]
fn test_test_demangle_stacks_noassert() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_demangle_stacks_noassert.wasm",
        "test_demangle_stacks_noassert",
        vec![],
        "../emscripten_resources/emtests/test_demangle_stacks_noassert.out"
    );
}
