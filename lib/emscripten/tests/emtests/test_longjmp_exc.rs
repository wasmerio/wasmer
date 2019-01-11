#[test]
#[ignore]
fn test_test_longjmp_exc() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp_exc.wasm",
        "test_longjmp_exc",
        vec![],
        "../../emtests/test_longjmp_exc.out"
    );
}
