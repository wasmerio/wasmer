#[test]
fn test_test_memmove3() {
    assert_emscripten_output!(
        "../../emtests/test_memmove3.wasm",
        "test_memmove3",
        vec![],
        "../../emtests/test_memmove3.out"
    );
}
