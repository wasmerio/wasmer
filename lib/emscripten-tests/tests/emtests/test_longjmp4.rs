#[test]
fn test_test_longjmp4() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp4.wasm",
        "test_longjmp4",
        vec![],
        "../../emtests/test_longjmp4.out"
    );
}
