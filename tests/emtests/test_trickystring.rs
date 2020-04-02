#[test]
#[ignore]
fn test_test_trickystring() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_trickystring.wasm",
        "test_trickystring",
        vec![],
        "../emscripten_resources/emtests/test_trickystring.out"
    );
}
