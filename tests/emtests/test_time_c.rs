#[test]
#[ignore]
fn test_test_time_c() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_time_c.wasm",
        "test_time_c",
        vec![],
        "../emscripten_resources/emtests/test_time_c.out"
    );
}
