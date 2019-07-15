#[test]
fn test_test_i64_llabs() {
    assert_emscripten_output!(
        "../../emtests/test_i64_llabs.wasm",
        "test_i64_llabs",
        vec![],
        "../../emtests/test_i64_llabs.out"
    );
}
