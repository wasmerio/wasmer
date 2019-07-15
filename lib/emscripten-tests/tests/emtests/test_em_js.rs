#[test]
#[ignore]
fn test_test_em_js() {
    assert_emscripten_output!(
        "../../emtests/test_em_js.wasm",
        "test_em_js",
        vec![],
        "../../emtests/test_em_js.out"
    );
}
