#[test]
#[ignore]
fn test_test_poll() {
    assert_emscripten_output!(
        "../../emtests/test_poll.wasm",
        "test_poll",
        vec![],
        "../../emtests/test_poll.out"
    );
}
