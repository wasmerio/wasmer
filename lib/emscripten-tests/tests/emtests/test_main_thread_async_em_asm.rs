#[test]
#[ignore]
fn test_test_main_thread_async_em_asm() {
    assert_emscripten_output!(
        "../../emtests/test_main_thread_async_em_asm.wasm",
        "test_main_thread_async_em_asm",
        vec![],
        "../../emtests/test_main_thread_async_em_asm.out"
    );
}
