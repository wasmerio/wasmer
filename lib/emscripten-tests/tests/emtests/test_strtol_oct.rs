#[test]
#[ignore]
fn test_test_strtol_oct() {
    assert_emscripten_output!(
        "../../emtests/test_strtol_oct.wasm",
        "test_strtol_oct",
        vec![],
        "../../emtests/test_strtol_oct.out"
    );
}
