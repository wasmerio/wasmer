#[test]
#[ignore]
fn test_test_getopt() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_getopt.wasm",
        "test_getopt",
        vec![],
        "../emscripten_resources/emtests/test_getopt.out"
    );
}
