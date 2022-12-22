# Wasm Interface

This is an experimental crate for validating the imports and exports of a WebAssembly module.

For the time being, Wasm Interface provides:

- a convenient text format for specifying the requirements of Wasm modules
- a convenient way to compose interfaces safely (it ensures no conflicts (duplicates are allowed but they must agree))
- validation that the modules meet the requirements

## Syntax example

Here's the interface for the current version of [WASI](https://github.com/WebAssembly/WASI):

```lisp
(interface "wasi_unstable"
  ;; Here's a bunch of function imports!
  (func (import "wasi_unstable" "args_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "args_sizes_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "clock_res_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "clock_time_get") (param i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "environ_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "environ_sizes_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_advise") (param i32 i64 i64 i32) (result i32))
  (func (import "wasi_unstable" "fd_allocate") (param i32 i64 i64) (result i32))
  (func (import "wasi_unstable" "fd_close") (param i32) (result i32))
  (func (import "wasi_unstable" "fd_datasync") (param i32) (result i32))
  (func (import "wasi_unstable" "fd_fdstat_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_fdstat_set_flags") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_fdstat_set_rights") (param i32 i64 i64) (result i32))
  (func (import "wasi_unstable" "fd_filestat_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_filestat_set_size") (param i32 i64) (result i32))
  (func (import "wasi_unstable" "fd_filestat_set_times") (param i32 i64 i64 i32) (result i32))
  (func (import "wasi_unstable" "fd_pread") (param i32 i32 i32 i64 i32) (result i32))
  (func (import "wasi_unstable" "fd_prestat_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_prestat_dir_name") (param i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_pwrite") (param i32 i32 i32 i64 i32) (result i32))
  (func (import "wasi_unstable" "fd_read") (param i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_readdir") (param i32 i32 i32 i64 i32) (result i32))
  (func (import "wasi_unstable" "fd_renumber") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_seek") (param i32 i64 i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_sync") (param i32) (result i32))
  (func (import "wasi_unstable" "fd_tell") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "fd_write") (param i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_create_directory") (param i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_filestat_get") (param i32 i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_filestat_set_times") (param i32 i32 i32 i32 i64 i64 i32) (result i32))
  (func (import "wasi_unstable" "path_link") (param i32 i32 i32 i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_open") (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_readlink") (param i32 i32 i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_remove_directory") (param i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_rename") (param i32 i32 i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_symlink") (param i32 i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "path_unlink_file") (param i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "poll_oneoff") (param i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "proc_exit") (param i32))
  (func (import "wasi_unstable" "proc_raise") (param i32) (result i32))
  (func (import "wasi_unstable" "random_get") (param i32 i32) (result i32))
  (func (import "wasi_unstable" "sched_yield") (result i32))
  (func (import "wasi_unstable" "sock_recv") (param i32 i32 i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "sock_send") (param i32 i32 i32 i32 i32) (result i32))
  (func (import "wasi_unstable" "sock_shutdown") (param i32 i32) (result i32))
)
```


Notes:
- multiple `assert-import` and `assert-export` declarations are allowed.
- comments (starts with `;` and ends with a newline) and whitespace are valid between any tokens

## Semantics

All imports used by the module must be specified in the interface.

All exports in the interface must be exported by the module.

Thus the module may have additional exports than the interface or fewer imports than the interface specifies and be considered valid.


## Misc

Wasm Interface serves a slightly different purpose than the proposed WebIDL for Wasm standard, but may be replaced by it in the future if things change.

Due to an issue with nested closures in Rust, `wasm-interface` can't both compile on stable and have good error reporting. This is being fixed and `wasm-interface` will be updated to have better error handling.

See the `parser.rs` file for a comment containing the grammar in a BNF style.

Suggestions, contributions, and thoughts welcome! This is an experiment in the early stages, but we hope to work with the wider community and develop this in cooperation with all interested parties.
