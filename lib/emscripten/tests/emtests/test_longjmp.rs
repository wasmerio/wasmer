#[test]
#[ignore]
fn test_test_longjmp() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp.wasm",
        "test_longjmp",
        vec![],
        "../../emtests/test_longjmp.out"
    );
}
