# libaio validation: static aliases and time64-only AIO

The pinned upstream libaio is `b8eadc9f89e8f7ab0338eacda9f98a6caea76883`.
No libaio sources have been edited. `userland-5.sh libaio` remains the full
static-library plus selected harness build; it currently fails for the second
reason below. `libaio-setup` is a separately labeled, narrower validation.

## Fixed: Wasm assembler rejects .symver

Use wasix-org/llvm-project branch `wasixv2/wasm-symver`, commit `568452b96`.
Wasm object emission now retains defined-symbol version-qualified names as
ordinary static aliases, and `@@` adds an unqualified alias for the default.
Function signatures, binding and visibility are retained after forward
symbols have been defined. Undefined sources and removal forms are rejected
explicitly. This adds no dynamic loader or runtime symbol-version lookup.

Build the branch normally with clang/lld and the WebAssembly target enabled,
then select that installation for this package only:

```sh
LIBAIO_LLVM_LOCATION=/absolute/path/to/llvm/build \
  bash userland-5.sh libaio
```

Focused failing-first MC tests check old/default function indices, data sizes,
default-name linking, diagnostics and existing alias/weak-alias behavior. The
actual unmodified `io_queue_wait.c` also assembles with its public alias.
No LLVM release or tag was published; existing shared branches stay unchanged.

## Still blocked: libaio assumes the legacy io_getevents syscall

`src/io_getevents.c` unconditionally expands `__NR_io_getevents`. In Linux
`include/uapi/asm-generic/unistd.h`, syscall 4 exists only for architectures
requesting time32 syscalls, or with a non-32-bit kernel-long ABI. WASIX v2 is
32-bit/time64-only: its generated headers and ABI expose
`io_pgetevents_time64` (416), without legacy syscall 4.

This is independent of `.symver`. The full build now reaches the missing
`__NR_io_getevents` compile error. Defining it to 416 is incorrect: the old
wrapper supplies five arguments, whereas the time64 syscall takes six,
including a signal-set pointer; the timeout layouts also differ. Adding an
invented number/import would change the ABI unnecessarily. No such alias,
preprocessor rewrite, fake architecture macro or syscall change is used.

Debian maintainer source `0ba8950f65b15381267552c732b7d12a9dd8adae` was also
inspected. Its time64 patch series still unconditionally builds the legacy
wrapper, and the full series disables the time64 syscall path. It is not a
ready no-source-change solution for a time64-only architecture.

The proper follow-up is an upstream Linux time64-only libaio port: use
`io_pgetevents_time64` for `io_getevents` with a NULL signal-set argument, convert
public timespec fields explicitly into `struct __kernel_timespec`, preserve
signal-mask and negative-errno behavior, and compile legacy fallbacks only when
their syscall numbers exist. Linux `fs/aio.c:SYSCALL_DEFINE6(io_pgetevents)` and
`do_io_getevents` define those semantics. Add a harness test with seconds above
INT32_MAX, invalid nanoseconds and signal-mask restoration before implementation.
This requires no kernel/exec contract change and should not be Wasm-specific.

## Narrow setup harness

The upstream harness Makefile documents `LIBAIO` as a link-input override.
`userland-5.sh libaio-setup` archives the real `src/io_setup.ol` in a standard `.a` and links unmodified
`harness/cases/2.t` against it. This tests invalid context addresses, invalid
counts, context creation and already initialized contexts. There is no substitute
library or implementation, and no implication that AIO event waiting is ported.

Run the official harness inside the guest with:

```sh
bash tests/run-programs-5.sh /absolute/path/to/wasmer libaio-setup
```

The full `libaio` workflow remains available for use after the upstream time64
port. Its broader selected harness cases have not been claimed as passing.
