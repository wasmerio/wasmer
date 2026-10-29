# Syscall-info-only strace port for WASIX v2

Inspected strace 6.17 (`9d7b4d1a59a20171bfad65a4eea883190ddd3791`),
WASIX v2 `0bc36eb981f`, and its ABI `07cd32e`. The existing build attempt in
`userland-5.sh strace` fails at configure's architecture dispatch. This document
is a port design, not a claim that strace builds or that `strace -f ls` passes.

The smallest sound port adds a native wasm32 personality and uses strace's
existing `PTRACE_GET_SYSCALL_INFO` path for syscall numbers, arguments, results,
IP and SP. It must never require GETREGSET/GETREGS, synthesize register contents,
or turn the kernel's EIO into a successful register read. Register-dependent
features need an explicit unsupported result; basic tracing and `-f` need none.

## Existing support to retain

- `src/syscall.c:strace_get_syscall_info`, `get_syscall_regs`, `get_scno`,
  `get_syscall_args` and `get_syscall_result` already decode the ptrace syscall
  info structure. The existing startup probe determines whether it is supported.
- `get_instruction_pointer` and `get_stack_pointer` already take their values
  from the info structure. WASIX v2 reports IP 0 and the guest shadow SP.
- Tracee memory goes through process_vm_readv/ptrace PEEKDATA; both exist for
  libc-built guests' shared linear memories. PTRACE_PEEKUSER/GETREGSET return EIO.
- Fork/clone/vfork/exec/exit events, TRACEME, SEIZE, INTERRUPT, SYSCALL and
  syscall-entry/exit stops exist. `-f` should use the normal event machinery.

## Architecture and scalar-slot work

Adding only a configure case or reusing riscv64's directory is incorrect.

1. Add `wasm32*` to `configure.ac`, its file list to `src/Makefile.am`, and a
   `src/linux/wasm32/` directory following the other native personalities.
   Keep ILP32 pointers, PEEK word size 4, and time64-only syscall tables.
2. Use the existing ABI's audit identity: `AUDIT_ARCH_RISCV32` (`0x400000f3`).
   `kernel/src/ptrace.rs:ptrace_get_syscall_info` and
   `codegen/wit/types.wit:audit-arch` explicitly specify it. This is a documented
   transport identity, not permission to reuse an unmodified RISC-V decoder.
   A distinct Wasm audit identity would require coordinated ABI/seccomp changes;
   that is outside this package and must be designed separately.
3. Preserve 64-bit syscall argument slots independently of 32-bit pointers and
   kernel-long structure fields. Currently `struct tcb.u_arg` and `u_rval` use
   `kernel_{u,}long_t`; on wasm32 these are 32-bit. `get_syscall_args` copies the
   info structure's uint64_t entries into that array, truncating them. Merely
   changing kernel_long_t to 64 bits changes structure layouts and PEEK behavior.
   Introduce a separate scalar-slot type/accessor (native defaults unchanged)
   plus unsigned/signed 64-bit result handling, with unit coverage above 4 GiB.
4. Derive the table from the WASIX ABI's numbers and typed argument order, using
   generic decoders only where their input layout matches. Concrete differences:
   syscall 62 is three-argument `lseek(fd, i64 offset, whence)`, not generic
   32-bit `_llseek(fd, hi, lo, result_pointer, whence)`; syscall 222 is `mmap`
   with a direct byte offset, not `mmap2` in page units. `pread64`, `pwrite64`,
   truncate/fallocate/fadvise and scalar 64-bit returns need the same audit.
   WASIX-only calls without Linux numbers can remain unnamed; no fake Linux
   numbers or invasive kernel tracing changes are needed for this workflow.

## Register-free control path

Add an architecture capability such as `ARCH_SYSCALL_INFO_ONLY` (default false)
to `arch_defs.h`. For that capability, make the startup probe mandatory and fail
clearly if syscall info is unavailable. A non-syscall startup/event stop may
return INFO_NONE: defer syscall decoding until the next actual syscall stop.
`get_syscall_regs` must not fall back to get_regs in that case. IP/SP fallbacks
return unavailable, not fabricated register values. Architecture fallback hooks
return EIO/unsupported. Syscall injection can use PTRACE_SET_SYSCALL_INFO later,
but is not needed for tracing; do not silently advertise working register edits.

## Required validation before publishing a fork branch

- Small upstream-style tests for INFO_ENTRY/EXIT/NONE handling and mandatory
  capability failure, with no GETREGSET request made on the info-only path.
- A fixture exercising lseek/pread and a scalar result beyond 32 bits; verify
  exact arguments and results, plus a low-address pointer and EFAULT.
- Guest `strace -f /bin/ls /work` and a shell that forks and execs ls, checking
  paths, getdents64, write, child pids and successful exit for every child.
- Basic signal and exec-event checks; unsupported `-k`/register features should
  fail explicitly. No changes to the kernel/exec contract are needed for these.

The scalar-slot/table audit makes this a real strace core/architecture port,
not a small register-backend addition. No partial port with truncated arguments
has been committed. The existing source build remains an honest architecture
probe until this work is implemented and tested in a strace fork.
