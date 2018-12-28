#[test]
#[ignore]
fn test_test_longjmp_stacked() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp_stacked.wasm",
        "test_longjmp_stacked",
        vec![],
        "../../emtests/test_longjmp_stacked.out"
    );
}
