#[test]
#[ignore]
fn test_getvalue_setvalue() {
    assert_emscripten_output!(
        "../../emtests/getValue_setValue.wasm",
        "getvalue_setvalue",
        vec![],
        "../../emtests/getValue_setValue.txt"
    );
}
