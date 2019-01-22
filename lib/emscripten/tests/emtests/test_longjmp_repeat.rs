#[test]
#[ignore]
fn test_test_longjmp_repeat() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp_repeat.wasm",
        "test_longjmp_repeat",
        vec![],
        "../../emtests/test_longjmp_repeat.out"
    );
}
