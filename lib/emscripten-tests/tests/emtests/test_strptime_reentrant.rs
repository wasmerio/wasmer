#[test]
#[ignore]
fn test_test_strptime_reentrant() {
    assert_emscripten_output!(
        "../../emtests/test_strptime_reentrant.wasm",
        "test_strptime_reentrant",
        vec![],
        "../../emtests/test_strptime_reentrant.out"
    );
}
