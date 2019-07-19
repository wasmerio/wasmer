#[test]
fn test_quine() {
    assert_wasi_output!(
        "../../wasitests/quine.wasm",
        "quine",
        vec![".".to_string(),],
        vec![],
        vec![],
        "../../wasitests/quine.out"
    );
}
