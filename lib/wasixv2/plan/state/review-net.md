# review-net (WP5 + WP6) state

Worktree: /home/theduke/dev/github.com/wasmerio/wasmer-review-net
- wasmer branch `wasixv2/review-net` (pushed): 70c542fa87b, one commit on bdcdb958bef
  "fix(wasixv2): host network bridge and Unix socket review fixes (WP5, WP6)".
- tests branch `review-net` (pushed, wasix-org/wasixv2-tests): 45b0716, 5 new netfix-* guest tests.
  Submodule pointer in the wasmer branch NOT bumped (left to merge/bless agent).

Gates passed: cargo test (vnet 43, vnet-host 4+12, kernel 188), clippy with lint RUSTFLAGS
(-D clippy::all --all-targets) on vnet, vnet-host, kernel, wasixv2, cargo fmt --all --check.
Suite filter `net` (release CLI): 195 passed, 0 failed, incl. the 5 new netfix tests.
LTP/posix socket tests not run separately (most listed socket ltp tests fail on Clone/fork ENOSYS).

## Remaining work
1. Split the single commit into logical commits (coordinator request, now optional).
   Prepared in logs/review-net/split/: zero-context hunks (h0.json, s0.txt summary, hunks.py),
   extracted test blocks (vnet_block.rs, knet_block.rs, reactor_tests.rs). Planned order:
   L-NET2 ports; M-NET4 uid; L-NET7/8 perf+dead code; M-NET6/L-NET1 disconnect/shutdown;
   L-NET6 accept4; L-NET4 netlink; H8 option replay (introduces vnet test fakes);
   M-NET1 accept gating/backlog; H7 datagram backpressure; M-NET2 recv error loop;
   M-NET3+L-NET3 cmsgs/UDP order; H9 unix inflight+gc; M-NET5 sockopt symmetry;
   M-NET5 peer creds/SO_PEERGROUPS; L-NET5 MSG_WAITALL creds.
   Mixed hunks needing per-stage fixups: unix.rs 51/66/78/82/99-101, net.rs 184, DESIGN 4,
   reactor 153 (listen fn + tests module); kernel sockopts test to be split in two.
2. Rebase onto the merge train (bin-commands/sigterm/userland-fixes) when it lands.
3. L-NET8 DESIGN "ping" sentence left to WP11 (only the dead-code part done).
4. Not done (judged out of scope or noted): pump holding StreamState across host syscalls
   (L-NET7, non-blocking syscalls), SO_*BUFFORCE for IP sockets (TODO in opts.rs),
   Unix SO_RCVLOWAT not honoured by recv, netlink unicast to other sockets.
