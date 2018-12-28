#[test]
fn test_test_atox() {
    assert_emscripten_output!(
        "../../emtests/test_atoX.wasm",
        "test_atox",
        vec![],
        "../../emtests/test_atoX.out"
    );
}
