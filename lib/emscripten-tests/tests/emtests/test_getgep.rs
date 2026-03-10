#[test]
fn test_test_getgep() {
    assert_emscripten_output!(
        "../../emtests/test_getgep.wasm",
        "test_getgep",
        vec![],
        "../../emtests/test_getgep.out"
    );
}
