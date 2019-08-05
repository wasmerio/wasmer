#[test]
fn test_test_i64_precise_needed() {
    assert_emscripten_output!(
        "../../emtests/test_i64_precise_needed.wasm",
        "test_i64_precise_needed",
        vec![],
        "../../emtests/test_i64_precise_needed.out"
    );
}
