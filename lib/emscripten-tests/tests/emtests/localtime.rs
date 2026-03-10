#[test]
fn test_localtime() {
    assert_emscripten_output!(
        "../../emtests/localtime.wasm",
        "localtime",
        vec![],
        "../../emtests/localtime.out"
    );
}
