# SQPOLL validation and exclusions

For pinned liburing `e907d6a342e80b70874f93abd440b92b8a40b7bc` (2.12),
`test/helpers.c:t_create_ring_params` skips setup only for `-EPERM` when the
caller is not root. It returns `-EINVAL` to the test. Both `read-write.t` and
`socket.t` require SQPOLL combinations and fail on that return.

`IORING_FEAT_SQPOLL_NONFIXED` describes whether an **already created SQPOLL
ring** accepts ordinary descriptors instead of registered files. `socket.c:recv_fn`
checks it only after successful setup. It is not a probe for SQPOLL availability.
No `IORING_FEAT_*` bit advertises basic SQPOLL support; it is a setup mode.

Linux has no separate build-time SQPOLL option under CONFIG_IO_URING.
`io_uring/sqpoll.c:io_sq_offload_create` creates or attaches the polling task;
security denial (`security_uring_sqpoll`) and historical capability checks can
return EPERM. `io_uring/io_uring.c:io_uring_setup` returns ENOSYS when all of
io_uring is compiled out, while its setup-flag validation returns EINVAL for
unsupported/invalid modes. There is no Linux configuration to emulate in which
ordinary rings work and a separately compiled-out SQPOLL mode returns a special
skip errno. Our documented unsupported mode remains EINVAL; claiming a permission
denial to trigger liburing's skip would misrepresent the cause.

The explicit exclusions are [liburing-excluded.txt](liburing-excluded.txt).
`tests/run-programs-5.sh <wasmer> liburing-supported` prints them and runs the
four supported follow-ups (poll, statx, unlink, mkdir). Raw `liburing` and
`liburing-followups` remain available to reproduce both unsupported failures.
The kernel does not advertise SQPOLL_NONFIXED, and no features/errno are changed
for a test harness.

Sources: Linux `io_uring/io_uring.c:io_uring_setup`,
`io_uring/sqpoll.c:io_sq_offload_create`, `include/uapi/linux/io_uring.h`;
liburing `test/helpers.c:t_create_ring_params`, `test/socket.c:recv_fn`.
