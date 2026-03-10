#[test]
fn test_test_addr_of_stacked() {
    assert_emscripten_output!(
        "../../emtests/test_addr_of_stacked.wasm",
        "test_addr_of_stacked",
        vec![],
        "../../emtests/test_addr_of_stacked.out"
    );
}
