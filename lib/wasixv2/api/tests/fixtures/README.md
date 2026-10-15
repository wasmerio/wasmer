# libc2 test binaries

Prebuilt programs for `tests/libc2.rs`, checked in because CI cannot build libc2. Keep them
small: they are built with `-Os` and stripped.

| File | Source |
| --- | --- |
| `hello.wasm` | `hello.c` |
| `smoke.wasm` | `wasix2/test/smoke.c` of libc2 |

Built with:

- libc2 commit `4b4d6bf8c0c79ac5a60da36a910daf191f274efc` (`v1.2.6-19-g4b4d6bf8`), sysroot from
  `wasix2/build-sysroot.sh` with target `wasm32-wasixv2`.
- wasix-org/llvm-project clang 22.1.8 (commit `3f3e05caa210706061b88ff694226d41d991c4c9`), which
  also provided compiler-rt for the builtins.
- wasm-tools 1.260.0.

To rebuild, build a sysroot as the libc2 README describes, then:

```sh
CC=<llvm-project>/build/bin/clang ./build.sh <sysroot-wasixv2> <libc2-checkout>
```

`build.sh` links each program with the `wasm32-wasixv2` driver
(`--target=wasm32-wasixv2 --no-default-config -Os -nodefaultlibs ... -lpthread -lc
libclang_rt.builtins.a`), strips it with `wasm-tools strip --all` and validates it. The binaries
import a shared `env.memory` and only the `wasixv2` functions they use.

If libc2's smoke test gains or loses checks, update the count that `libc2_smoke_test_passes`
expects.
