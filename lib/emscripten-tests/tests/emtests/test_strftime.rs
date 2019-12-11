#[test]
#[ignore]
fn test_test_strftime() {
    assert_emscripten_output!(
        "../../emtests/test_strftime.wasm",
        "test_strftime",
        vec![],
        "../../emtests/test_strftime.out"
    );
}
