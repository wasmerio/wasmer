# Userland 6: real-program validation

This extends userland 5 with source-only build recipes and bounded guest workflows.
Builds stay under `$USERLAND6` (default `~/wasixv2-tools/userland-6`). All target
programs use wasixcc's **asyncify** sysroot, including servers and shell helpers.
Tarballs are pinned by SHA256 in `userland-6.sources`; libfuse uses the same
3.17.4 commit as userland 5. No Wasm binaries belong in this repository.

Source the plan's `env.sh`, select libc2 `b969f9aa` / ABI `07cd32e` with
`WASIXCC_SYSROOT_PREFIX`, and put host Meson, Ninja, Bison and Flex on PATH.
`userland-6.sh host-flex` can build Flex into the private `$USERLAND6/host`
prefix. Put that prefix's `bin` on PATH before building PostgreSQL.

Build one package per invocation, under the shared-machine limits:

```sh
nice -n19 taskset -c 4-7 bash userland-6.sh dash
nice -n19 taskset -c 4-7 bash userland-6.sh curl
nice -n19 taskset -c 4-7 bash userland-6.sh wget
nice -n19 taskset -c 4-7 bash userland-6.sh socat
nice -n19 taskset -c 4-7 bash userland-6.sh redis
WASMER=/absolute/release/wasmer nice -n19 taskset -c 4-7 bash userland-6.sh nginx
```

Other package names: `postgres`, `busybox`, `libfuse`, `fuse-overlayfs`, `glib`,
`sshfs`. Build libfuse before the two FUSE applications. GLib needs a target
PCRE2 and libffi; this kit does not port either library. Nginx's compiler wrapper
runs its target configure probes through the CLI, so sizes and feature results
come from the actual guest. Busybox's linker adapter consumes the compiler-only
`-nostdlib` flag before calling wasm-ld for partial links. Its x86 SHA accelerators
and hardware-only `hwclock` and obsolete CBQ `tc` applets are disabled through Kconfig.

The runner combines the existing source-built userland-4 tools with the new
outputs and uses the source-built userland-5 strace port. Override `USERLAND4`
and `STRACE_WASM` to select other builds. Build the shell pty helper first:

```sh
WASIXCC_TARGET=wasm32-wasixv2 WASIXCC_WASM_EXCEPTIONS=no \
  nice -n19 taskset -c 4-7 wasixcc -O2 -g tests/repros/shell-pty.c \
  -o "$HOME/wasixv2-tools/userland-6/out/bin/shell-pty"
bash tests/run-programs-6.sh /absolute/release/wasmer dash
```

Workflows: `dash`, `bash`, `nginx`, `redis`, `postgres`, `overlay`, `network`,
`netns`, `coreutils`, `busybox`. Each has an independent host timeout and a log
under `$USERLAND6/logs`. Host panics fail validation even if the CLI returns zero.
Server/network workflows use `strace -f`, with traces under `$USERLAND6/work`.
Nginx and PostgreSQL keep service state in guest `/tmp`; host-backed directories
cannot be chowned by guest root. PostgreSQL uses static libpq/client targets and
mounts system timezone data at `/zones`; no plugin/shared-library port is included.

`coreutils` selects sixteen upstream shell tests, with their standard test
initialization and fd 9. `busybox` runs its complete upstream `runtest -v` for
all built applets, subject to the bounded host timeout. Neither invokes the WASIX
conformance suite. Rclone is skipped because it needs Go and no target Wasm build
was provided. Gdb is outside this package's scope; use the wasm32 strace port.

Validated on runtime `b178e7a7b16` (base `f2e457967c8`), libc2 `b969f9aa`,
ABI `07cd32e`. Runtime validation used only filtered kernel tests: 30 ptrace,
35 process and 56 task tests; all passed, along with kernel clippy using the
Makefile flags, formatting and the systemd release CLI build.

| Program | Result |
| --- | --- |
| dash 0.5.12 / Bash 5.3 | Guest pty Ctrl-Z, jobs, bg, fg, second stop, resume and reap pass. Bash comes from the existing source-built userland-4 kit. |
| nginx 1.28.0 | Two workers, HTTP response, SIGHUP worker replacement and changed response, graceful shutdown pass under strace. |
| Redis 7.0.15 | SET/GET benchmark (200 each, 4 clients), BGSAVE and retrieval pass under strace. |
| curl 8.14.1 / wget 1.25.0 | HTTP through the builtin DNS server passes. TLS/NTLM are disabled build features. |
| socat 1.8.0.3 | Guest TCP transfer passes, including between namespaces. |
| iproute2 7.3.0 / iputils | Two namespaces and a veth pair carry ICMP and TCP. Reuses existing source-built tools. |
| Coreutils 9.12 | Sixteen selected upstream tests: 15 pass, one skips because it requires non-root. |
| Busybox 1.37.0 | Complete driver for built applets: 951 pass, 12 fail, 23 skip, one untested. See failure classification below. |
| fuse-overlayfs 1.15 / libfuse 3.17.4 | Both build; daemon rejects missing FUSE_CAP_DONT_MASK at init. Mount workflow fails. |
| sshfs 3.7.3 | Build blocked by absent target GLib. GLib 2.84 configure lacks PCRE2 and mandates libffi; excluded dependency work was not attempted. No sshd workflow ran. |
| PostgreSQL 17.5 | Server and requested clients build statically. Bootstrap succeeds; post-bootstrap initialization needs the missing dict_snowball plugin. initdb and pgbench do not complete. |
| rclone / gdb | Skipped as requested: no supplied Go Wasm build / no wasm32 gdb target. |

The Redis trace exposed a kernel bug: execution release removed traced thread
zombies before the tracer consumed their exits, losing their tids and leaving
strace with ECHILD. The fix retains each until both execution and tracer release
it, preserves Linux delay_group_leader, and publishes group-killed thread exits
after memory cleanup. The kernel regression failed first with ECHILD; the fixed
Redis trace contains all worker exits and strace exits zero. No ABI or kernel/exec
contract change was needed.

Busybox failure classification (all failures remain visible):

- `cpio lists hardlinks` is explicitly marked as currently failing in upstream
  cpio.tests, including the two missing hardlink names observed here.
- `awk gsub erroneous word start match`, `date-timezone`, and sed's embedded-NUL,
  NUL-in-command and nonexistent-label cases concern userspace parsing/regex
  behavior. They were not established as kernel bugs.
- `hostname-i-works` compares net-tools' IPv6 result with Busybox's IPv4 result.
- Two loop-mount cases need loop block devices and their filesystems.
- `start-stop-daemon with both -x and -a` explicitly cannot use a multicall
  /bin/false; the existing kit provides coreutils as a multicall program.
- `strings-works-like-GNU` lacks its GNU strings reference; `unzip (subdir only)`
  lacks the zip fixture generator.

Larger follow-ups:

1. Proper FUSE_DONT_MASK needs original create mode and umask carried through the
   VFS/driver boundary for create/mkdir/mknod, negotiated at FUSE init, including
   ACL interaction. Current NewNode already has the masked mode. Linux references:
   fs/fuse/inode.c:fuse_send_init/process_init_reply and
   fs/fuse/dir.c:fuse_create_open/fuse_mkdir. Advertising the bit alone is wrong.
2. PostgreSQL's standard SQL requires C plugins (dict_snowball, later plpgsql).
   That crosses the excluded dynamic-loading work; no replacement/stub was added.
   SysV shmget/shmat and semget/semctl/semop are exercised before that boundary. The build selects SysV
   semaphores through PostgreSQL's documented PREFERRED_SEMAPHORES override.
3. Redis 7.2.7 crashes LLVM's MC assembler on the forced common definitions in
   redismodule.h included by tls.c. No module API or compiler logic was changed;
   unmodified Redis 7.0.15 avoids that connection-module refactor.
4. An initial coreutils run emitted an intermittent host panic, “syscall from an
   unknown task,” while still returning zero. Eight focused reproduction runs and
   the final subset did not reproduce it. No root cause or fix is claimed. The
   runner now rejects such a panic irrespective of CLI status.

Durable machine-local evidence is under `~/wasixv2-orch/logs/v9progs/STATE.md`
and its build/run logs. No expected-failure list was modified and no full WASIX
conformance suite was run.
