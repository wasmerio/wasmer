#[test]
fn test_envvar() {
    assert_wasi_output!(
        "../../wasitests/envvar.wasm",
        "envvar",
        vec![],
        vec![],
        "../../wasitests/envvar.out"
    );
}
