#[test]
#[ignore]
fn test_test_strtold() {
    assert_emscripten_output!(
        "../../emtests/test_strtold.wasm",
        "test_strtold",
        vec![],
        "../../emtests/test_strtold.out"
    );
}
