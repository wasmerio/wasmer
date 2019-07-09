#[test]
#[ignore]
fn test_test_ccall() {
    assert_emscripten_output!(
        "../../emtests/test_ccall.wasm",
        "test_ccall",
        vec![],
        "../../emtests/test_ccall.out"
    );
}
