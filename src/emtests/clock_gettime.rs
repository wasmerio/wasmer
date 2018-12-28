#[test]
fn test_clock_gettime() {
    assert_emscripten_output!(
        "../../emtests/clock_gettime.wasm",
        "clock_gettime",
        vec![],
        "../../emtests/clock_gettime.out"
    );
}
