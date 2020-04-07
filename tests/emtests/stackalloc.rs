#[test]
#[ignore]
fn test_stackalloc() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/stackAlloc.wasm",
        "stackalloc",
        vec![],
        "../emscripten_resources/emtests/stackAlloc.txt"
    );
}
