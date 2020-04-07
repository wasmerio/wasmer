#[test]
#[ignore]
fn test_test_longjmp_exc() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_longjmp_exc.wasm",
        "test_longjmp_exc",
        vec![],
        "../emscripten_resources/emtests/test_longjmp_exc.out"
    );
}
