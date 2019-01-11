#[test]
#[ignore]
fn test_stackalloc() {
    assert_emscripten_output!(
        "../../emtests/stackAlloc.wasm",
        "stackalloc",
        vec![],
        "../../emtests/stackAlloc.txt"
    );
}
