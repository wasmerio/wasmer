#[test]
#[ignore]
fn test_test_getopt_long() {
    assert_emscripten_output!(
        "../../emtests/test_getopt_long.wasm",
        "test_getopt_long",
        vec![],
        "../../emtests/test_getopt_long.out"
    );
}
