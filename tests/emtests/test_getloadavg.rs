#[test]
#[ignore]
fn test_test_getloadavg() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_getloadavg.wasm",
        "test_getloadavg",
        vec![],
        "../emscripten_resources/emtests/test_getloadavg.out"
    );
}
