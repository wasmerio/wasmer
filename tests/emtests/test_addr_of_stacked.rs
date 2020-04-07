#[test]
fn test_test_addr_of_stacked() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_addr_of_stacked.wasm",
        "test_addr_of_stacked",
        vec![],
        "../emscripten_resources/emtests/test_addr_of_stacked.out"
    );
}
