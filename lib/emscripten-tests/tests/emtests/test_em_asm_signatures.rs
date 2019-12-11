#[test]
#[ignore]
fn test_test_em_asm_signatures() {
    assert_emscripten_output!(
        "../../emtests/test_em_asm_signatures.wasm",
        "test_em_asm_signatures",
        vec![],
        "../../emtests/test_em_asm_signatures.out"
    );
}
