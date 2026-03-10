#[test]
#[ignore]
fn test_test_trickystring() {
    assert_emscripten_output!(
        "../../emtests/test_trickystring.wasm",
        "test_trickystring",
        vec![],
        "../../emtests/test_trickystring.out"
    );
}
