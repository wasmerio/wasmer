#[test]
fn test_test_array2b() {
    assert_emscripten_output!(
        "../../emtests/test_array2b.wasm",
        "test_array2b",
        vec![],
        "../../emtests/test_array2b.out"
    );
}
