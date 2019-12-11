#[test]
#[ignore]
fn test_test_getopt() {
    assert_emscripten_output!(
        "../../emtests/test_getopt.wasm",
        "test_getopt",
        vec![],
        "../../emtests/test_getopt.out"
    );
}
