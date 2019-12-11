#[test]
#[ignore]
fn test_test_utf() {
    assert_emscripten_output!(
        "../../emtests/test_utf.wasm",
        "test_utf",
        vec![],
        "../../emtests/test_utf.out"
    );
}
