#[test]
#[ignore]
fn test_test_strtoll_hex() {
    assert_emscripten_output!(
        "../../emtests/test_strtoll_hex.wasm",
        "test_strtoll_hex",
        vec![],
        "../../emtests/test_strtoll_hex.out"
    );
}
