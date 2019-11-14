#[test]
#[ignore]
fn test_test_perrar() {
    assert_emscripten_output!(
        "../../emtests/test_perrar.wasm",
        "test_perrar",
        vec![],
        "../../emtests/test_perrar.out"
    );
}
