#[test]
#[ignore]
fn test_test_indirectbr_many() {
    assert_emscripten_output!(
        "../../emtests/test_indirectbr_many.wasm",
        "test_indirectbr_many",
        vec![],
        "../../emtests/test_indirectbr_many.out"
    );
}
