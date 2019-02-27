#[test]
fn test_getcwd() {
    assert_emscripten_output!(
        "../../emtests/test_getcwd.wasm",
        "getcwd",
        vec![],
        "../../emtests/test_getcwd.out"
    );
}
