#[test]
#[ignore]
fn test_test_em_asm_signatures() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_em_asm_signatures.wasm",
        "test_em_asm_signatures",
        vec![],
        "../emscripten_resources/emtests/test_em_asm_signatures.out"
    );
}
