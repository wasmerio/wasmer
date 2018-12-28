#[test]
#[ignore]
fn test_test_siglongjmp() {
    assert_emscripten_output!(
        "../../emtests/test_siglongjmp.wasm",
        "test_siglongjmp",
        vec![],
        "../../emtests/test_siglongjmp.out"
    );
}
