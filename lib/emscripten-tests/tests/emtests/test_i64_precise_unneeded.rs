#[test]
fn test_test_i64_precise_unneeded() {
    assert_emscripten_output!(
        "../../emtests/test_i64_precise_unneeded.wasm",
        "test_i64_precise_unneeded",
        vec![],
        "../../emtests/test_i64_precise_unneeded.out"
    );
}
