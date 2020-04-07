#[test]
#[ignore]
fn test_test_strptime_reentrant() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strptime_reentrant.wasm",
        "test_strptime_reentrant",
        vec![],
        "../emscripten_resources/emtests/test_strptime_reentrant.out"
    );
}
