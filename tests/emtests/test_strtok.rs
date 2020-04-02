#[test]
#[ignore]
fn test_test_strtok() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_strtok.wasm",
        "test_strtok",
        vec![],
        "../emscripten_resources/emtests/test_strtok.out"
    );
}
