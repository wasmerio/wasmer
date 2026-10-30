# Real programs 3 (V11)

Base runtime: feat/wasixv2 6a8410a5a45; libc2 6bc05b3b; ABI 07cd32e;
wasixv2-tests dff5da8. This kit extends userland-6/7 without modifying their
source trees. Exact source URLs and SHA256 pins are in userland-9.sources.
Build output and downloaded sources stay outside git in ~/wasixv2-tools/userland-9.
No prebuilt Wasm is checked in.

Source the plan env.sh with BINARYEN set, then select the exact-base libc2
sysroot with WASIXCC_SYSROOT_PREFIX. All builds use wasixcc, static libraries
and the asyncify sysroot (WASIXCC_WASM_EXCEPTIONS=no). Build packages one at a
time with `nice -n 19 taskset -c 4-7 bash userland-9.sh PACKAGE`; JOBS defaults
to 3. Build dependency order: zlib before git and openssh; libevent and ncurses
before tmux; openssl before openssh. Build the CLI with cranelift in a separate
systemd user unit before running anything. Set WASIXV2_PROBE_CLI to it for Perl.

Run one workflow with `bash tests/run-programs-9.sh RELEASE_CLI PROGRAM`.
It mounts the previously source-built userland/out/bin helpers, this kit's
binaries and source-only workflow files. Data lives in guest tmpfs. The driver
retains syscall traces under userland-9/work and rejects host panics even if the
CLI exits zero. Build tmux's pty driver from tests/repros/tmux-pty.c with wasixcc.
Unpack the pinned Lua test archive to userland-9/lua-tests.

Workflows:

- SQLite: create/insert/select, WAL reader concurrent with a held write
  transaction, a competing writer denied by the lock, post-commit read, VACUUM,
  .backup and integrity_check. The WAL shared-memory file stays on guest tmpfs.
- Git: init, add, commits on diverging branches, merge, log, gc/fsck, local
  clone and a post-commit shell hook. No curl/HTTPS, plugins or scripting tools.
- GNU make: eight shell-built files, four parallel slots and a recursive make
  inheriting the pipe jobserver; joined file contents and warnings checked.
- tmux: controlling v2 pty, server socket, new session, split, send-keys,
  capture-pane, detach, reattach another client and normal server shutdown.
- rsync: -aH permissions/mtime/symlink/hardlink checks, --delete, then the same
  data and hardlinks transferred over loopback to its daemon.
- Lua: eight exact upstream test files without the C test module or dlopen.
- Perl: static miniperl, actual target configure probes through the CLI, then
  selected bundled base/arithmetic/fork/pipe/stat tests. No dynamic extensions.
- OpenSSH: static libcrypto/zlib, sshd and its two helpers, key-only loopback
  login, remote command and scp through the ordinary SFTP subsystem.

Results are pending. Full chronological evidence and active-unit names are in
~/wasixv2-orch/logs/v11progs/STATE.md. Package/source configuration blockers are
recorded separately from Linux kernel/libc bugs; no behavior workaround or
expected-failure change is permitted.
