#[test]
#[ignore]
fn test_test_dlmalloc_partial_2() {
    assert_emscripten_output!(
        "../../emtests/test_dlmalloc_partial_2.wasm",
        "test_dlmalloc_partial_2",
        vec![],
        "../../emtests/test_dlmalloc_partial_2.out"
    );
}
