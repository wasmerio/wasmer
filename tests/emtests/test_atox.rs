#[test]
fn test_test_atox() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_atoX.wasm",
        "test_atox",
        vec![],
        "../emscripten_resources/emtests/test_atoX.out"
    );
}
