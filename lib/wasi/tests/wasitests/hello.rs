#[test]
fn test_hello() {
    assert_wasi_output!(
        "../../wasitests/hello.wasm",
        "hello",
        vec![],
        vec![],
        "../../wasitests/hello.out"
    );
}
