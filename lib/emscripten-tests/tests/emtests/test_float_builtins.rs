#[test]
#[ignore]
fn test_test_float_builtins() {
    assert_emscripten_output!(
        "../../emtests/test_float_builtins.wasm",
        "test_float_builtins",
        vec![],
        "../../emtests/test_float_builtins.out"
    );
}
