# libaio validation on time64-only Linux targets

The kit pins upstream libaio `b8eadc9f89e8f7ab0338eacda9f98a6caea76883`.
Its upstream master still has no time64-only port. The Debian maintainer's
separate time64 series was also inspected; it still assumes a legacy
io_getevents wrapper and disables its time64 syscall path, so it does not solve
this target's build.

## Generic source port

[patches/libaio/time64-only.patch](patches/libaio/time64-only.patch) is a
Linux port selected by syscall availability, with no WASIX architecture test or
invented syscall number. On a target without legacy `__NR_io_getevents`, the
public wrapper calls `io_pgetevents` with a NULL signal mask. When only
`__NR_io_pgetevents_time64` exists, that wrapper explicitly converts public
`struct timespec` fields to two signed 64-bit kernel fields, packages the
signal mask normally, and preserves libaio's negative-errno convention.
Existing legacy-syscall wrappers remain in use where available. This is a
port for time64-only targets, rather than a general dual-ABI glibc time64 port.

The generic syscall helper must not invent legacy syscall 4 when libc omits
it (for example riscv32). A compile regression checks this. Linux
`fs/aio.c:do_io_getevents`, `SYSCALL_DEFINE6(io_pgetevents)` and
`kernel/time/time.c:get_timespec64` define the corresponding behavior.

New upstream-style cases 24 and 25 check invalid min_nr, noncanonical
nanoseconds, seconds above INT32_MAX, unchanged timeout storage and errno;
case 24 also checks pending-signal EINTR and temporary-mask restoration.
Linux AIO normalizes nanoseconds through timespec64_to_ktime; it does not
reject tv_nsec=1000000000. The fixtures preserve that distinction.

The patch also makes the complete old harness build on modern musl/Clang:
standard C99 field designators, poll.h, eventfd2 feature detection and standard
large-file interfaces with _FILE_OFFSET_BITS=64. These are source-portability
changes; existing test assertions are unchanged. `userland-5.sh libaio` builds
the entire static library and all 23 harness binaries, including the new two.
The run workflow selects nine cases (2, 3, 4, 5, 12, 15, 22, 24, 25), never the
full suite. Repeated builds verify the already-applied patch without resetting
other source edits.

## Toolchain

Static Wasm aliases for libaio's existing .symver directives require
wasix-org/llvm-project branch `wasixv2/wasm-symver`, commit `568452b96`.
That earlier LLVM change retains defined-symbol version-qualified names as
ordinary static aliases; @@ also adds the unqualified default. No symbol
version directive is changed by this port, and no runtime linker work is added.

Build and run in an owned output directory, under the shared-machine limits:

```sh
USERLAND5=/absolute/owned/output \
LIBAIO_LLVM_LOCATION=/absolute/path/to/patched/llvm \
  bash userland-5.sh libaio
USERLAND5=/absolute/owned/output \
  bash tests/run-programs-5.sh /absolute/path/to/wasmer libaio
```

## Evidence and dependencies

The original full library failed on missing __NR_io_getevents. An independently
linked old io_pgetevents wrapper fails case 24 with ENOSYS instead of Linux's
EINVAL for invalid min_nr. With the port, both new cases pass on native Linux
and inside WASIX. The complete static library and all harness binaries build,
and the second build succeeds without reapplying or discarding source edits.

Case 4's NULL write buffer also needs the ordinary kernel-copy guard fix from
V1 (wasmer 5266fa8f3c5). A CLI still based on train 18 passes eight selected
cases but fails that independent guard assertion. The combined V1+V7 proof
runs are recorded in the validation agent's STATE.md; no per-test pointer
exception or ABI workaround belongs in libaio.

The older `libaio-setup` workflow remains as a separately labeled narrow probe:
it links the real upstream io_setup object through the documented LIBAIO link
input, without implying broader event-wait coverage.
