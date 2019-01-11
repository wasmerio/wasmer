#[test]
#[ignore]
fn test_test_longjmp2() {
    assert_emscripten_output!(
        "../../emtests/test_longjmp2.wasm",
        "test_longjmp2",
        vec![],
        "../../emtests/test_longjmp2.out"
    );
}
