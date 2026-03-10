#[test]
#[ignore]
fn test_test_em_asm_unused_arguments() {
    assert_emscripten_output!(
        "../../emtests/test_em_asm_unused_arguments.wasm",
        "test_em_asm_unused_arguments",
        vec![],
        "../../emtests/test_em_asm_unused_arguments.out"
    );
}
