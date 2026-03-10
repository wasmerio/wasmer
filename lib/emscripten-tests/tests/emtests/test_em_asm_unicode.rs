#[test]
#[ignore]
fn test_test_em_asm_unicode() {
    assert_emscripten_output!(
        "../../emtests/test_em_asm_unicode.wasm",
        "test_em_asm_unicode",
        vec![],
        "../../emtests/test_em_asm_unicode.out"
    );
}
