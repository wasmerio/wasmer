#[test]
#[ignore]
fn test_test_demangle_stacks() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_demangle_stacks.wasm",
        "test_demangle_stacks",
        vec![],
        "../emscripten_resources/emtests/test_demangle_stacks.out"
    );
}
