# V15 static real programs — complete

Runtime `wasixv2/v15progs` f1532f50687 (base b145215a694), tests `v15progs`
3a5d1a0 (base ed854e9), libc2 `v15progs` 4101c810 (base 6bc05b3b), ABI
unchanged 07cd32e. All commits are AI-attributed and pushed fast-forward.
Source kits and final binaries: `~/wasixv2-tools/userland-11`.
Full logs, RED evidence, source/hash manifests and state:
`~/wasixv2-orch/logs/v15progs`. No binaries are committed.

## Program results

| Program | Actual final guest validation |
| --- | --- |
| CPython 3.13.3 | Static builtins, no dlopen: requested regrtest subset **3233 PASS /0 FAIL /271 natural skips**; 113 UDP-Lite cases explicitly not selected after original unsupported-protocol collection abort |
| OpenSSL 3.5.0 + curl 8.14.1 | Verified self-signed localhost certificate, HTTP/1.1 payload equality and connection reuse against source-built nginx 1.28.0 inside the same machine; nginx connection request counter reaches2; SHA256/AES128-CBC speed1s each |
| FFmpeg 7.1.1 | Generate3 video frames plus audio, encode FFV1/PCM Matroska, pipe into MPEG2-video/MP2 MPEG-TS, pipe into full decode/re-encode to null; both streams nonempty; no hardware acceleration |
| Vim 9.1.1354 | Real guest /dev/ptmx controlling slave, scripted insertion and :wq, saved-file content check |
| htop 3.4.1 | Real guest PTY, CPU/memory/task display, named driver and pthread rows, /proc Threads=2 and nonzero CPU accounting |
| strace -f | Static Python starts/joins a thread, runs a subprocess, forks/reaps, then SIGKILLs a subprocess and asserts wait=-SIGKILL; real trace confirms kill/CLD_KILLED, no host panic |

All five workflows pass on the final systemd-built release CLI. Full source-built
Python profile uses Modules/Setup.local plus detected static Setup.stdlib;
Nine unsupported/shared-only module names, including _ctypes/FFI, are disabled explicitly.
The final target Makefile has DYNLOADFILE=dynload_stub.o and no shared modules.
Same-version native Python builds only explicitly static generator prerequisites.
Other program profiles select ordinary supported static options, not function stubs.

## Python selection and counts

Original upstream assertions and JUnit testcase records provide the counts;
pilots and pre-fix runs are not added. Default regrtest resources remain disabled.

| Group | PASS | FAIL | Natural SKIP |
| --- | ---: | ---: | ---: |
| `test_asyncio.test_events` | 261 | 0 | 0 |
| `test_asyncio.test_futures` | 181 | 0 | 0 |
| `test_asyncio.test_tasks` | 984 | 0 | 1 |
| `test_fcntl` | 9 | 0 | 2 |
| `test_mmap` | 38 | 0 | 12 |
| `test_multiprocessing_fork` | 334 | 0 | 68 |
| `test_os` | 308 | 0 | 58 |
| `test_pty` | 6 | 0 | 0 |
| `test_select` | 6 | 0 | 0 |
| `test_signal` | 52 | 0 | 5 |
| `test_socket` | 548 | 0 | 79 |
| `test_subprocess` | 299 | 0 | 43 |
| `test_threading` | 207 | 0 | 3 |
| **Total** | **3233** | **0** | **271** |

`test_asyncio` is bounded to futures/tasks/events as requested basics. Initial
unfiltered `test_socket` aborts because its UDP-Lite gate only checks the header
constant; the machine returns EPROTONOSUPPORT for that unavailable protocol.
The opt-in `PYTHON11_IGNORE_UDPLITE=1` uses upstream --ignore '*UDPLITE*';
--list-cases enumerates113 excluded cases, distinct from271 natural skips.
The remaining627 socket cases all pass, including32 previously failing IPv6
ancillary assertions. No expected files or xfail baseline was changed.

## Genuine defects and failing-first evidence

1. libc2 pathconf/fpathconf returned constant NAME_MAX without checking the file.
   New sibling suite/base-pathconf-name-max fails on the exact base (EXIT134),
   then passes with both EH and asyncify sysroots and native Linux glibc.
   Fix4101c810 derives _PC_NAME_MAX from existing statvfs/fstatvfs. Linux
   fs/statfs.c:fd_statfs/user_statfs and glibc sysdeps/posix/{fpathconf,pathconf}.c
   define the behavior. Original upstream test_os failure is now green.
2. UDP queues discarded packet metadata; recvmsg returned empty IPv6 controls.
   New sibling vnet queued-packet/peek/options/override/legacy regression is
   RED(EXIT101) before production edits. Parser assertion independently proves
   parsed IPV6_DONTFRAG was discarded. Fixf1532f50687 preserves class/flags and
   arrival metadata and emits controls in Linux net/ipv6/datagram.c:
   ip6_datagram_recv_ctl order, with ip6_datagram_send_ctl and udp.c:
   udpv6_sendmsg override/default behavior. Original32 failures all now pass.

Gates:32 targeted vnet UDP/cmsg/multicast/raw/BPF checks,2 runner fixture parser
checks, both-config guest libc regression and native control, exact Makefile lint
flags/all-target clippy for vnet and runner, cargo fmt --all --check PASS.
No full WASIX/workspace/conformance suite ran; the merge train owns it.

## Reproduction

Source archives and SHA256 pins are in userland-11.sources; source-only builders
are userland-11.sh. Set BINARYEN, WASIXCC_SYSROOT_PREFIX to the fixed libc2's
build-wasixv2 and WASIXV2_PROBE_CLI to the release CLI. Build packages serially:
openssl, curl, ncurses, ffmpeg, vim, htop, python-native, python, nginx.
The Python recipe retains Lib/data/generated sysconfig under python-stdlib so
intermediate build trees can be removed. Its relink case is for unchanged-ABI
libc updates only. Build the PTY harness tests/repros/programs-pty-11.c with
wasixcc -O2 -pthread, asyncify, into out/bin/programs-pty.

Use tests/run-programs-11.sh CLI {tls,ffmpeg,vim,htop,python-strace}; use
PYTHON11_IGNORE_UDPLITE=1 tests/python-11.sh CLI for the13-group selection, or
pass exact group names after CLI for focused confirmation. Python uses the
existing16MiB CLI stack setting and1 compiler thread after initial1MiB stack
exhaustion/shared-host OOM evidence; other workflows use up to3 compiler threads.
Always use nice19, CPUs4-7, make/cargo-j3, CARGO_INCREMENTAL=0, one heavy job at
a time and systemd user units for long jobs. Snapshot shell recipe text before
launching if editing concurrently; never edit a guest script while it executes.

Source kits inherited from earlier packages are preserved. Final programs and
static dependencies are retained; only owned intermediate build trees and target
are removed after hash/byte-identity checks (completed;17GiB free). Invalid wrapper/configuration runs,
original failures and host-OOM evidence remain in logs and are not counted.

Existing boundaries: incoming host-backend IP-header ancillary metadata is not
available in that backend interface; this fix covers queued in-machine packets
without inventing host header values. UDP-Lite remains unavailable. Optional
FFI/shared-only extensions and unselected Python tests are not claimed validated.
No dynamic linking, libffi, closures, reflection, ABI or kernel/exec contract was
changed. Complete per-binary hashes and per-group results are in
userland-11.results.json and the local artifact manifest.
