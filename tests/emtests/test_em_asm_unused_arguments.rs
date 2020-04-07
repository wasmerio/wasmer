#[test]
#[ignore]
fn test_test_em_asm_unused_arguments() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_em_asm_unused_arguments.wasm",
        "test_em_asm_unused_arguments",
        vec![],
        "../emscripten_resources/emtests/test_em_asm_unused_arguments.out"
    );
}
