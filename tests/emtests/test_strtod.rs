#[test]
#[ignore]
fn test_test_strtod() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtod.wasm",
        "test_strtod",
        vec![],
        "../emscripten_resources/emtests/test_strtod.out"
    );
}
