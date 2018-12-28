#[test]
#[ignore]
fn test_test_em_asm_2() {
    assert_emscripten_output!(
        "../../emtests/test_em_asm_2.wasm",
        "test_em_asm_2",
        vec![],
        "../../emtests/test_em_asm_2.out"
    );
}
