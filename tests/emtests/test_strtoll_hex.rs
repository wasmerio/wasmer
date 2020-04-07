#[test]
#[ignore]
fn test_test_strtoll_hex() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtoll_hex.wasm",
        "test_strtoll_hex",
        vec![],
        "../emscripten_resources/emtests/test_strtoll_hex.out"
    );
}
