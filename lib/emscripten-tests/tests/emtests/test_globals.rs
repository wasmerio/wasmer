#[test]
fn test_test_globals() {
    assert_emscripten_output!(
        "../../emtests/test_globals.wasm",
        "test_globals",
        vec![],
        "../../emtests/test_globals.out"
    );
}
