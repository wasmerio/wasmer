#[test]
fn test_fseek() {
    assert_wasi_output!(
        "../../wasitests/fseek.wasm",
        "fseek",
        vec![],
        vec![],
        "../../wasitests/fseek.out"
    );
}
