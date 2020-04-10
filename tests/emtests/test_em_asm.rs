#[test]
#[ignore]
fn test_test_em_asm() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_em_asm.wasm",
        "test_em_asm",
        vec![],
        "../emscripten_resources/emtests/test_em_asm.out"
    );
}
