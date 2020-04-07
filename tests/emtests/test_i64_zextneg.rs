#[test]
fn test_test_i64_zextneg() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_zextneg.wasm",
        "test_i64_zextneg",
        vec![],
        "../emscripten_resources/emtests/test_i64_zextneg.out"
    );
}
