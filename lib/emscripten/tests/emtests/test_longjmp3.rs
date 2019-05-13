#[test]
fn test_test_longjmp3() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp3.wasm",
        "test_longjmp3",
        vec![],
        "../../emtests/test_longjmp3.out"
    );
}
