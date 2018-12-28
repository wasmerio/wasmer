#[test]
#[ignore]
fn test_test_getloadavg() {
    assert_emscripten_output!(
        "../../emtests/test_getloadavg.wasm",
        "test_getloadavg",
        vec![],
        "../../emtests/test_getloadavg.out"
    );
}
