#[test]
#[ignore]
fn test_test_time_c() {
    assert_emscripten_output!(
        "../../emtests/test_time_c.wasm",
        "test_time_c",
        vec![],
        "../../emtests/test_time_c.out"
    );
}
