#[test]
#[ignore]
fn test_test_longjmp_throw() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp_throw.wasm",
        "test_longjmp_throw",
        vec![],
        "../../emtests/test_longjmp_throw.out"
    );
}
