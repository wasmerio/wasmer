#[test]
#[ignore]
fn test_test_strtod() {
    assert_emscripten_output!(
        "../../emtests/test_strtod.wasm",
        "test_strtod",
        vec![],
        "../../emtests/test_strtod.out"
    );
}
