#[test]
#[ignore]
fn test_test_gmtime() {
    assert_emscripten_output!(
        "../../emtests/test_gmtime.wasm",
        "test_gmtime",
        vec![],
        "../../emtests/test_gmtime.out"
    );
}
