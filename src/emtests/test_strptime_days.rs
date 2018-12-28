#[test]
#[ignore]
fn test_test_strptime_days() {
    assert_emscripten_output!(
        "../../emtests/test_strptime_days.wasm",
        "test_strptime_days",
        vec![],
        "../../emtests/test_strptime_days.out"
    );
}
