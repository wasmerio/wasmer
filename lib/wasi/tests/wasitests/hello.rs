#[test]
fn test_hello() {
    assert_wasi_output!(
        "../../wasitests/hello.wasm",
        "hello",
        vec![],
        "../../wasitests/hello.out"
    );
}
