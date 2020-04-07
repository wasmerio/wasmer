#[test]
#[ignore]
fn test_test_fast_math() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_fast_math.wasm",
        "test_fast_math",
        vec![],
        "../emscripten_resources/emtests/test_fast_math.out"
    );
}
