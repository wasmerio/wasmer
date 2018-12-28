#[test]
fn test_test_strtok() {
    assert_emscripten_output!(
        "../../emtests/test_strtok.wasm",
        "test_strtok",
        vec![],
        "../../emtests/test_strtok.out"
    );
}
