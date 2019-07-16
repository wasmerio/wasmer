#[test]
fn test_envvar() {
    assert_wasi_output!(
        "../../wasitests/envvar.wasm",
        "envvar",
        vec![],
        vec!["DOG=1".to_string(), "CAT=2".to_string(),],
        "../../wasitests/envvar.out"
    );
}
