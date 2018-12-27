#[test]
fn test_test_alloca() {
    assert_emscripten_output!(
        "../../emtests/test_alloca.wasm",
        "test_alloca",
        vec![],
        "../../emtests/test_alloca.out"
    );
}
