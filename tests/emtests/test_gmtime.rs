#[test]
#[ignore]
fn test_test_gmtime() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_gmtime.wasm",
        "test_gmtime",
        vec![],
        "../emscripten_resources/emtests/test_gmtime.out"
    );
}
