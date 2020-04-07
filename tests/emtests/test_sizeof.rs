#[test]
#[ignore]
fn test_test_sizeof() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_sizeof.wasm",
        "test_sizeof",
        vec![],
        "../emscripten_resources/emtests/test_sizeof.out"
    );
}
