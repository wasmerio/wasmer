#[test]
fn test_test_i64_precise_unneeded() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_precise_unneeded.wasm",
        "test_i64_precise_unneeded",
        vec![],
        "../emscripten_resources/emtests/test_i64_precise_unneeded.out"
    );
}
