#[test]
fn test_test_longjmp_unwind() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp_unwind.wasm",
        "test_longjmp_unwind",
        vec![],
        "../../emtests/test_longjmp_unwind.out"
    );
}
