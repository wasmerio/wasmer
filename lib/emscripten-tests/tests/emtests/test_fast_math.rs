#[test]
#[ignore]
fn test_test_fast_math() {
    assert_emscripten_output!(
        "../../emtests/test_fast_math.wasm",
        "test_fast_math",
        vec![],
        "../../emtests/test_fast_math.out"
    );
}
