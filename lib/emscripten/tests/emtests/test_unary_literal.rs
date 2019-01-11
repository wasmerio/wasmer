#[test]
fn test_test_unary_literal() {
    assert_emscripten_output!(
        "../../emtests/test_unary_literal.wasm",
        "test_unary_literal",
        vec![],
        "../../emtests/test_unary_literal.out"
    );
}
