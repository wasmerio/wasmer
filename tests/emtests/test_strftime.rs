#[test]
#[ignore]
fn test_test_strftime() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strftime.wasm",
        "test_strftime",
        vec![],
        "../emscripten_resources/emtests/test_strftime.out"
    );
}
