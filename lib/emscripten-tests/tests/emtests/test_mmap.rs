#[test]
#[ignore]
fn test_test_mmap() {
    assert_emscripten_output!(
        "../../emtests/test_mmap.wasm",
        "test_mmap",
        vec![],
        "../../emtests/test_mmap.out"
    );
}
