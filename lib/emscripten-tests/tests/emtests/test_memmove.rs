#[test]
fn test_test_memmove() {
    assert_emscripten_output!(
        "../../emtests/test_memmove.wasm",
        "test_memmove",
        vec![],
        "../../emtests/test_memmove.out"
    );
}
