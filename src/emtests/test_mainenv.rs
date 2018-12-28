#[test]
#[ignore]
fn test_test_mainenv() {
    assert_emscripten_output!(
        "../../emtests/test_mainenv.wasm",
        "test_mainenv",
        vec![],
        "../../emtests/test_mainenv.out"
    );
}
