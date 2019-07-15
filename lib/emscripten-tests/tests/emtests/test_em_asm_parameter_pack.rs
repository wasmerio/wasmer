#[test]
#[ignore]
fn test_test_em_asm_parameter_pack() {
    assert_emscripten_output!(
        "../../emtests/test_em_asm_parameter_pack.wasm",
        "test_em_asm_parameter_pack",
        vec![],
        "../../emtests/test_em_asm_parameter_pack.out"
    );
}
