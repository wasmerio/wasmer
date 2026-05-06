wasm_test!(test_epoll_create, "epoll-create");
wasm_test!(test_poll_oneoff_zero, "poll-oneoff-zero");
wasm_test!(test_poll_oneoff, "poll-oneoff");

wasm_test!(
    test_epoll_create_ctl_wait,
    "epoll-create-ctl-wait",
    stdout = "0"
);
