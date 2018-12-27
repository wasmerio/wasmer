#[test]
fn test_puts() {
    assert_emscripten_output!(
        "../../emtests/puts.wasm",
        "puts",
        vec![],
        "../../emtests/puts.out"
    );
}
