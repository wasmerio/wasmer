#[test]
#[ignore]
fn test_test_mmap() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_mmap.wasm",
        "test_mmap",
        vec![],
        "../emscripten_resources/emtests/test_mmap.out"
    );
}
