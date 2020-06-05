# uname only works in *Unix like systems
ifneq ($(OS), Windows_NT)
  ARCH := $(shell uname -m)
  UNAME_S := $(shell uname -s)
else
  # We can assume, if in windows it will likely be in x86_64
  ARCH := x86_64
  UNAME_S := 
endif

compilers :=

# Singlepass is enabled
RUST_VERSION := $(shell rustc -V)

ifneq (, $(findstring nightly,$(RUST_VERSION)))
  # Singlepass doesn't work yet on Windows
  ifneq ($(OS), Windows_NT)
    compilers += singlepass
  endif
endif

ifeq ($(ARCH), x86_64)
  # In X64, Cranelift is enabled
  compilers += cranelift
  # LLVM could be enabled if not in Windows
  ifneq ($(OS), Windows_NT)
    # Autodetect LLVM from llvm-config
    ifneq (, $(shell which llvm-config))
      LLVM_VERSION := $(shell llvm-config --version)
      # If findstring is not empty, then it have found the value
      ifneq (, $(findstring 10,$(LLVM_VERSION)))
        compilers += llvm
      endif
    else
      ifneq (, $(shell which llvm-config-10))
        compilers += llvm
      endif
    endif
  endif
endif

compilers := $(filter-out ,$(compilers))

ifneq ($(OS), Windows_NT)
  bold := $(shell tput bold)
  green := $(shell tput setaf 2)
  reset := $(shell tput sgr0)
endif


$(info Available compilers: $(bold)$(green)${compilers}$(reset))

compiler_features_spaced := $(foreach compiler,$(compilers),$(compiler))
compiler_features := --features "$(compiler_features_spaced)"


############
# Building #
############

build-wasmer:
	cargo build --release $(compiler_features)

WAPM_VERSION = v0.5.0
build-wapm:
	git clone --branch $(WAPM_VERSION) https://github.com/wasmerio/wapm-cli.git
	cargo build --release --manifest-path wapm-cli/Cargo.toml --features "telemetry update-notifications"

build-docs:
	cargo doc --release --all-features --document-private-items --no-deps

build-docs-capi:
	cd lib/c-api/ && doxygen doxyfile

# We use cranelift as the default backend for the capi for now
build-capi: build-capi-cranelift

build-capi-singlepass:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,wasi

build-capi-cranelift:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features cranelift-backend,wasi

build-capi-llvm:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features llvm-backend,wasi


###########
# Testing #
###########

test:
	cargo test --release $(compiler_features)

test-capi-singlepass: build-capi-singlepass
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features singlepass,wasi

test-capi-cranelift: build-capi-cranelift
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features cranelift,wasi -- --nocapture --test-threads=1

test-capi-llvm: build-capi-llvm
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features llvm,wasi

test-capi: test-capi-singlepass test-capi-cranelift test-capi-llvm test-capi-emscripten

#############
# Packaging #
#############

package-wasmer:
	# This command doesn't build the binary, just packages it
	mkdir -p ./package/bin
ifeq ($(OS), Windows_NT)
	cp ./target/release/wasmer.exe ./package/bin/
else
	cp ./target/release/wasmer ./package/bin/
endif
  # Comment WAPM for now to speedup release process
	# cp ./wapm-cli/target/release/wapm ./package/bin/
	# # Create the wax binary as symlink to wapm
	# cd ./package/bin/ && ln -sf wapm wax && chmod +x wax

package-capi:
	# This command doesn't build the C-API, just packages it
	mkdir -p ./package/
	mkdir -p ./package/include
	mkdir -p ./package/lib
ifeq ($(OS), Windows_NT)
	cp target/release/wasmer_c_api.dll ./package/lib/wasmer.dll
	cp target/release/wasmer_c_api.lib ./package/lib/wasmer.lib
else
ifeq ($(UNAME_S), Darwin)
	cp target/release/libwasmer_c_api.dylib ./package/lib/libwasmer.dylib
	cp target/release/libwasmer_c_api.a ./package/lib/libwasmer.a
	# Fix the rpath for the dylib
	install_name_tool -id "@rpath/libwasmer.dylib" ./package/lib/libwasmer.dylib
else
	cp target/release/libwasmer_c_api.so ./package/lib/libwasmer.so
	cp target/release/libwasmer_c_api.a ./package/lib/libwasmer.a
endif
endif
	find target/release/build -name 'wasmer.h*' -exec cp {} ./package/include ';'
	cp lib/c-api/doc/index.md ./package/include/README.md

package-docs: build-docs build-docs-capi
	mkdir -p package/docs
	mkdir -p package/docs/c
	cp -R target/doc package/docs/crates
	cp -R lib/c-api/doc/html package/docs/c-api
	echo '<!-- Build $(SOURCE_VERSION) --><meta http-equiv="refresh" content="0; url=rust/wasmer_runtime/index.html">' > package/docs/index.html
	echo '<!-- Build $(SOURCE_VERSION) --><meta http-equiv="refresh" content="0; url=wasmer_runtime/index.html">' > package/docs/crates/index.html

package: package-wasmer package-capi
	cp LICENSE ./package/LICENSE
	cp ATTRIBUTIONS.md ./package/ATTRIBUTIONS
ifeq ($(OS), Windows_NT)
  iscc wasmer.iss
else
	tar -C ./package -zcvf wasmer.tar.gz bin lib include LICENSE ATTRIBUTIONS
endif

#################
# Miscellaneous #
#################

# Updates the spectests from the repo
update-testsuite:
	git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

RUSTFLAGS := "-D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects" # TODO: add `-D missing-docs`
lint:
	cargo fmt --all -- --check
	RUSTFLAGS=${RUSTFLAGS} cargo clippy $(compiler_features)

install-local: package
	tar -C ~/.wasmer -zxvf wasmer.tar.gz

publish-docs:
	git clone -b "gh-pages" --depth=1 https://wasmerbot:$(GITHUB_DOCS_TOKEN)@github.com/wasmerio/wasmer.git api-docs-repo
	cp -R package/docs/* api-docs-repo/
	cd api-docs-repo && git add index.html crates/* c-api/*
	cd api-docs-repo && (git diff-index --quiet HEAD || git commit -m "Publishing GitHub Pages")
	# cd api-docs-repo && git push origin gh-pages
