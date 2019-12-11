#[test]
fn test_test_array2() {
    assert_emscripten_output!(
        "../../emtests/test_array2.wasm",
        "test_array2",
        vec![],
        "../../emtests/test_array2.out"
    );
}
