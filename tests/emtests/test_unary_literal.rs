#[test]
#[ignore]
fn test_test_unary_literal() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_unary_literal.wasm",
        "test_unary_literal",
        vec![],
        "../emscripten_resources/emtests/test_unary_literal.out"
    );
}
