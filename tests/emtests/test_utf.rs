#[test]
#[ignore]
fn test_test_utf() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_utf.wasm",
        "test_utf",
        vec![],
        "../emscripten_resources/emtests/test_utf.out"
    );
}
