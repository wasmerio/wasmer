#[test]
#[ignore]
fn test_test_i64_7z() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i64_7z.wasm",
        "test_i64_7z",
        vec![],
        "../emscripten_resources/emtests/test_i64_7z.out"
    );
}
