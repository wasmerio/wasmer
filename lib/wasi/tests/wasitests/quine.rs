#[test]
fn test_quine() {
    assert_wasi_output!(
        "../../wasitests/quine.wasm",
        "quine",
        vec![],
        "../../wasitests/quine.out"
    );
}
