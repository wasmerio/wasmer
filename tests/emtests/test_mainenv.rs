#[test]
#[ignore]
fn test_test_mainenv() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_mainenv.wasm",
        "test_mainenv",
        vec![],
        "../emscripten_resources/emtests/test_mainenv.out"
    );
}
