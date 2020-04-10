#[test]
#[ignore]
fn test_test_em_asm_parameter_pack() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_em_asm_parameter_pack.wasm",
        "test_em_asm_parameter_pack",
        vec![],
        "../emscripten_resources/emtests/test_em_asm_parameter_pack.out"
    );
}
