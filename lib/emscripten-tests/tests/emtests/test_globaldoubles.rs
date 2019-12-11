#[test]
fn test_test_globaldoubles() {
    assert_emscripten_output!(
        "../../emtests/test_globaldoubles.wasm",
        "test_globaldoubles",
        vec![],
        "../../emtests/test_globaldoubles.out"
    );
}
