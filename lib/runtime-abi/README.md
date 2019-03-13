# runtime-abi

This crate has ABI functions (like syscalls) and extensions to the runtime for enabling ABIs (e.g. virtual filesystem).

## Virtual Filesystem (experimental)

The virtual filesystem allows the runtime to read bundled wasm data as if they were files. Data that is stored in a 
custom section compressed with [zstd][1] compression and archived with [tar][2] will be exposed as files and mounted
in the `/` root.

The only current supported operation is the `read` syscall. 

The virtual filesystem is not enabled by default. Build with `--features vfs` to use it. 

[Zbox][3] is a virtual filesystem that depends on [libsodium][4]. See [installation instructions][5] for libsodium here. One can
statically link libsodium with the [instructions][6] on Zbox's readme. 

[1]: https://facebook.github.io/zstd/
[2]: https://www.gnu.org/software/tar/
[3]: https://zbox.io/
[4]: https://download.libsodium.org/doc/
[5]: https://download.libsodium.org/doc/installation
[6]: https://github.com/zboxfs/zbox#static-linking-with-libsodium
