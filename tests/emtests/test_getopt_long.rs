#[test]
#[ignore]
fn test_test_getopt_long() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_getopt_long.wasm",
        "test_getopt_long",
        vec![],
        "../emscripten_resources/emtests/test_getopt_long.out"
    );
}
