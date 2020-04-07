#[test]
#[ignore]
fn test_test_std_cout_new() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_std_cout_new.wasm",
        "test_std_cout_new",
        vec![],
        "../emscripten_resources/emtests/test_std_cout_new.out"
    );
}
