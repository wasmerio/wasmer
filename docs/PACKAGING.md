# Wasmer OS distro packaging notes

* Wasmer is written in Rust. To build Wasmer, where possible, do not
  directly invoke `cargo`, but use the supplied `Makefile`

* Wasmer provides several compilers and the `Makefile` autodetects
  when compilers can be compiled and/or installed. Set the environment
  variables `ENABLE_{CRANELIFT,LLVM,SINGLEPASS}=1` to force compiler
  to be build or to fail trying, e.g:

  ```sh
  $ ENABLE_LLVM=1 make build-wasmer
  ```

* `make install` respects `DESTDIR`, but `prefix` must be configured
  with `WASMER_INSTALL_PREFIX`. Note that `DESTDIR` must include
  `WASMER_INSTALL_PREFIX`, e.g.:

  ```sh
  export WASMER_INSTALL_PREFIX=/usr
  make
  DESTDIR=/tmp/staging/usr make install
  ```

* In case you must build/install directly with `cargo`, make sure to
  enable at least one compiler feature, like e.g. `--features
  cranelift`,

  * Beware that compiling with `cargo build --workspace --features …`
    will not enable features on the subcrates in the workspace and
    result in a headless Wasmer binary that can not compile Wasm files
    directly.

* If you split the package into several subpackages, beware that the
  `create-exe` command of the `wasmer` CLI requires `libwasmer.a` to
  be installed at `$WASMER_INSTALL_PREFIX/lib/libwasmer.a`. Suggestions for splitting:

  * The `wasmer-headless` CLI contains a subset of the `wasmer`'s functionalities
    and should only be packaged when splitting — it must be built
    explicitly with:
    
    ```sh
    $ make build-wasmer-headless-minimal install-wasmer-headless-minimal
    ```
  * `libwasmer`, containing `libwasmer.so*`,
  * `libwasmer-dev`, containing the header files and a `.pc` file,
  * `libwasmer-static`, containing `libwasmer.a`.

The Wasmer distro packaging story is still in its infancy, so feedback is very welcome.

## Miscellaneous: binfmt_misc

Wasmer can be registered as a binfmt interpreter for wasm binaries.
An example systemd [.service](./scripts/wasmer-binfmt.service.example) is included here.
Please consider statically linking the wasmer binary so that this capability is also available in mount namespaces.
