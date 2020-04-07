#[test]
#[ignore]
fn test_test_siglongjmp() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_siglongjmp.wasm",
        "test_siglongjmp",
        vec![],
        "../emscripten_resources/emtests/test_siglongjmp.out"
    );
}
