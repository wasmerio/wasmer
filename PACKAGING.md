## Wasmer distro packaging notes

* Where possible, do not directly invoke cargo, but use the supplied Makefile
	* wasmer has several compiler backends and the Makefile autodetects whether to enable llvm and singlepass.
	  Set `ENABLE_{CRANELIFT,LLVM,SINGLEPASS}=1` to build the full set or fail trying
	* Set `WASMER_CAPI_USE_SYSTEM_LIBFFI=1` to force dynamic linking of libffi on the shared library
	* `make install` respects `DESTDIR`, but `prefix` must be configured as e.g. `WASMER_INSTALL_PREFIX=/usr make all`
* In case you must build/install directly with cargo, make sure to enable at least one compiler backend feature
  * Beware that compiling with `cargo build --workspace/--all --features ...` will not enable features on the subcrates in the workspace and result in a headless wasmer binary that can not run wasm files directly.
* If you split the package into several subpackages, beware that the create-exe command of wasmer requires `libwasmer.a` to be installed at `$WASMER_INSTALL_PREFIX/lib/libwasmer.a`.
  Suggestion for splitting:
  * `wasmer` and `wasmer-headless`, containing the respective executables
    * `wasmer-headless` contains a subset of `wasmer`'s functionality and should only be packaged when splitting - it must be built explicitly with `make build-wasmer-headless-minimal insteall-wasmer-headless-minimal`
  * `libwasmer`, containing `libwasmer.so*`
  * `libwasmer-dev`, containging the header files and a `.pc` file
  * `libwasmer-static`, containing `libwasmer.a`

The wasmer distro packaging story is still in its infancy, so feedback is very welcome.
