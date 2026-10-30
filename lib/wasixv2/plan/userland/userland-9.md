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
systemd user unit before running anything. Set WASIXV2_PROBE_CLI to it for Perl and the executed zlib probe.

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
  capture-pane, detach, reattach another client, detach it and shut down the server.
  Upstream tmux intentionally returns 1 to attached clients on server shutdown.
- rsync: -aH permissions/mtime/symlink/hardlink checks, --delete, then the same
  data and hardlinks transferred over loopback to its daemon.
- Lua: eight exact upstream test files without the C test module or dlopen,
  using the upstream _soft and _port modes.
- Perl: static miniperl, actual target configure probes through the CLI, then
  twelve selected bundled base/arithmetic/fork/pipe/stat tests. No dynamic
  extensions. Preserve Configure's -fno-strict-aliasing when adding _GNU_SOURCE;
  generate the standard Config.pm. The copied harness includes TestInit.pm and
  mounts the source paths used by buildcustomize.pl. PERL_SKIP_TTY_TEST=1 uses
  the upstream opt-out for four checks requiring a controlling tty.
- OpenSSH: static libcrypto/zlib, sshd and its two helpers, key-only loopback
  login, remote command and scp through the ordinary SFTP subsystem. Scp uses
  its standard -S /bin/ssh option because binaries are mounted at /bin.

Build details: Git uses its documented NO_REGEX=NeedsStartEnd selection for
musl; tmux uses the existing private host Bison. OpenSSH's old unprototyped
configure probes generate Wasm signature-mismatch trap stubs that Binaryen 129
cannot parse. A configure-only RAND_add patch and real-header typed-symbol
link probes derive crypto cache values from actual target links; a real zlib
compression/decompression program is compiled and run before setting its cache
value. No library implementation, crypto behavior or excluded subsystem is
modified. OpenSSH's native hardening/retpoline/rpath flags are disabled through
its supported configure options for this wasm target.

Seven programs pass their complete requested workflows on the baseline runtime.
Optimized miniperl builds and executes its selected tests. The traced fork tests
exposed a real kernel panic: after SIGKILL prevents a syscall stop, ptrace_stop
returns SIGTRAP|0x80, which the internal signal sender failed to validate before
indexing the disposition table. Linux kernel/signal.c:send_sig_info rejects that
value. A failing-first kernel unit test covers both entry and exit notifications;
the small sender validation fix and final program rerun are being gated.

Full chronological evidence and active-unit names are in
~/wasixv2-orch/logs/v11progs/STATE.md. Package/source configuration blockers are
recorded separately from Linux kernel/libc bugs; no behavior workaround or
expected-failure change is permitted.
