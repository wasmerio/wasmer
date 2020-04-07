#[test]
#[ignore]
fn test_test_strptime_days() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strptime_days.wasm",
        "test_strptime_days",
        vec![],
        "../emscripten_resources/emtests/test_strptime_days.out"
    );
}
