# wasixv2 definitions

The Linux syscall ABI for wasm32 guests, written as annotated WIT. `wasixv2-codegen` reads these
files with `wit-parser` only; nothing here uses the component model.

- `types.wit` (`interface types`): scalar and pointer aliases, kernel structs and unions, and
  constant families.
- `syscalls.wit` (`interface syscalls`): one function per riscv32 syscall.
- `ext.wit` (`interface ext`): wasm-specific primitives.

Both function interfaces use the single import module `wasixv2`. Import names are the snake form
of the function names (`exit_group`, `startup_get`), so a syscall and an extension must not share
a name; the generator rejects collisions.

These files are the source of truth. Edit them by hand.

## ABI

- riscv32 is the reference: asm-generic, ILP32, time64 only. Its struct layouts are
  byte-identical to wasm32.
- Syscalls mirror Linux 1:1: the riscv32 table name is the import name, and the arguments and
  semantics are the same. The return value is a Linux `long`, either a value or `-errno`.
- Arguments that riscv32 splits into register pairs are a single `s64`/`u64` parameter here.
  This also covers `llseek` (`offset_high`, `offset_low`) and the `pos_l`/`pos_h` pair of
  `preadv`/`pwritev`/`preadv2`/`pwritev2`. `mmap2` keeps its offset in 4096-byte units.
- Records describe the layout the kernel sees, because that is what the host implements.
  `@c` names the type that musl passes to the syscall, where one matches.
- Four syscalls are omitted: `lookup_dcookie` (18) and `nfsservctl` (42), which are
  `sys_ni_syscall` stubs, and the riscv-specific `riscv_hwprobe` (258) and
  `riscv_flush_icache` (259). Defining `SYS_riscv_flush_icache` would make musl build its
  riscv-only icache code.

## Naming

- Identifiers are kebab-case. Converting kebab to snake gives the Linux C name (`stx-mode` ->
  `stx_mode`), and converting to SCREAMING_SNAKE gives a constant name (`o-creat` -> `O_CREAT`).
- WIT keywords are escaped with `%` (`%flags`, `%type`, `%list`).
- A Linux name that kebab-case cannot represent keeps its exact spelling in `@c-name(...)` on
  fields and `@name(...)` on constants, for example `__WALL` or `_NSIG`.
- Kernel-internal names drop their leading underscores. For example, `__kernel_timespec`
  becomes `kernel-timespec`.
- Pointer aliases are named `T-ptr` (mutable), `const-T-ptr`, `T-array` and `const-T-array`.

## Annotations

wit-parser turns every comment directly in front of an item into that item's docs. A doc line
whose trimmed text starts with `@` is an annotation. All other lines are free text. Unknown
annotations are an error.

A line starting with `====` is a section divider. The divider comments
(`// ==== Sockets ====`) attach to the first item of each section, and generators drop them.

| Annotation | On | Meaning |
| --- | --- | --- |
| `@c(<C type>)` | alias, record, enum, flags | The C type musl uses, for example `@c(struct statx)` or `@c(pid_t)`. |
| `@kernel-only` | record | musl has no layout-compatible type. Either musl's type differs and musl converts it (`rusage`, `msqid64-ds`), or musl's type is a larger prefix-compatible superset (`termios`, `sysinfo`, `sched-param`, `sigset`). Do not assert this layout against musl. |
| `@pointer-sized` | alias | The width is the target pointer width. WIT declares the wasm32 width. |
| `@newtype` | alias of an integer | Rust backends emit a distinct `#[repr(transparent)]` struct (`pub struct Fd(pub i32)`) instead of a type alias, so values of this type cannot be mixed up with plain integers. Used on `fd`. C sees the plain integer. |
| `@ptr(const T)`, `@ptr(mut T)` | alias of `uptr` | A guest pointer to `T`. `T` can be a type name, a primitive, `void`, or `T[]` for an array. |
| `@nullable` | pointer alias | The pointer may be NULL. Not used yet: nullability is per argument, so the function docs describe it. |
| `@union` | record | All fields are at offset 0. |
| `@size(N)`, `@align(N)` | record | The riscv32 layout. Generators assert it. Every record has both. |
| `@repr(T)` | enum, flags | The integer type of the constant family. |
| `@abi-error` | enum | The errno family whose values syscalls return negated (`-errno`). At most one enum has it, and its `@repr` must be signed. Used on `errno`. |
| `@value(N)` | enum case, flags member | The value, in decimal or `0x` hex, and possibly negative. Flags members carry the full mask, not a bit index. Aliases may share a value. |
| `@name(C_NAME)` | enum case, flags member | The exact C name, when it is not the SCREAMING_SNAKE form of the case name. |
| `@c-name(c_name)` | record field | The exact C field name, when it is not the snake form of the field name. |
| `@c-skip` | record field, enum case, flags member | No C assert for this item: musl's type has no such member, or musl intentionally uses a different value (and translates). The docs say why. |
| `@c-enum` | enum, flags | musl defines the members as C enumerators, not macros. Their C asserts are not guarded by `#ifdef`. |
| `@module(name)` | interface | The wasm import module. |
| `@nr(N)` | syscall | The riscv32 syscall number. |
| `@sys-name(name)` | syscall | The musl `SYS_` suffix, when it differs from the import name (`_llseek`). |
| `@merged-i64(param[, hi-lo])` | syscall | Linux passes `param` as two 32-bit halves. The default order is `lo-hi`. |
| `@noreturn` | function | The function never returns, so it has no result (`exit`, `exit_group`, `rt_sigreturn`). |
| `@unsupported` | function | Declared for completeness. Hosts may return `-ENOSYS`. |
| `@c-param(name: C type)` | function | Overrides the C prototype type of one parameter. |
| `@import(name)` | function | Overrides the wasm import name. By default it is the snake form of the function name. |
| `@slice(ptr, len)` | function | `ptr` is a byte buffer (`u8` or `void` pointee) and `len` its unsigned length in bytes. The Rust `Syscalls` method takes one `UserSlice` (const pointee) or `UserSliceMut` (mut pointee) named after `ptr` instead of the two params. Only for lengths that count bytes of exactly that buffer; signed lengths (`readlinkat`'s `bufsiz`) stay separate because a negative value means `EINVAL`. Repeatable. |
| `@returns(T)` | function | The Rust `Syscalls` method returns `SysResult<T>` instead of the raw result type, for example `u32` for byte counts or `fd` for new descriptors. `T` is a scalar no wider than the result; the import shim converts it back. |

The Rust backends require every function without a result to be `@noreturn`; its `Syscalls`
method returns `SysResult<()>` and the import shim traps if it returns to the guest.

Functions cannot document individual parameters, because wit-parser drops comments on
parameters. Notes about a parameter go in the function docs. A command-dependent argument is
typed `ulong`, and the function docs explain it.

## Validation

`wasixv2-codegen` parses these files, computes C layouts and checks every `@size`/`@align`.
The definitions were cross-checked against clang record-layout dumps of the Linux UAPI headers
and of musl's riscv32 headers.
