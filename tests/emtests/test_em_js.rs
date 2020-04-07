#[test]
#[ignore]
fn test_test_em_js() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_em_js.wasm",
        "test_em_js",
        vec![],
        "../emscripten_resources/emtests/test_em_js.out"
    );
}
