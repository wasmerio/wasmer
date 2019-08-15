#[test]
fn test_poll_oneoff() {
    assert_wasi_output!(
        "../../wasitests/poll_oneoff.wasm",
        "poll_oneoff",
        vec![],
        vec![(
            "hamlet".to_string(),
            ::std::path::PathBuf::from("wasitests/test_fs/hamlet")
        ),],
        vec![],
        "../../wasitests/poll_oneoff.out"
    );
}
