#[test]
#[ignore]
fn test_test_indirectbr_many() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_indirectbr_many.wasm",
        "test_indirectbr_many",
        vec![],
        "../emscripten_resources/emtests/test_indirectbr_many.out"
    );
}
