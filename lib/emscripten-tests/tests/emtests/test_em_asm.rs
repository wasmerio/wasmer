#[test]
#[ignore]
fn test_test_em_asm() {
    assert_emscripten_output!(
        "../../emtests/test_em_asm.wasm",
        "test_em_asm",
        vec![],
        "../../emtests/test_em_asm.out"
    );
}
