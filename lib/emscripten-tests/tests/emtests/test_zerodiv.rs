#[test]
#[ignore]
fn test_test_zerodiv() {
    assert_emscripten_output!(
        "../../emtests/test_zerodiv.wasm",
        "test_zerodiv",
        vec![],
        "../../emtests/test_zerodiv.out"
    );
}
