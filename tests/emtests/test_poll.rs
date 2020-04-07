#[test]
#[ignore]
fn test_test_poll() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_poll.wasm",
        "test_poll",
        vec![],
        "../emscripten_resources/emtests/test_poll.out"
    );
}
