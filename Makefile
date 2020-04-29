.PHONY: spectests emtests clean build install lint precommit docs examples 

# uname only works in *Unix like systems
ifneq ($(OS), Windows_NT)
  ARCH := $(shell uname -m)
  UNAME_S := $(shell uname -s)
else
  # We can assume, if in windows it will likely be in x86_64
  ARCH := x86_64
  UNAME_S := 
endif

backends :=

# Singlepass is enabled
RUST_VERSION := $(shell rustc -V)

ifneq (, $(findstring nightly,$(RUST_VERSION)))
  # Singlepass doesn't work yet on Windows
  ifneq ($(OS), Windows_NT)
    backends += singlepass
  endif
endif

ifeq ($(ARCH), x86_64)
  # In X64, Cranelift is enabled
  backends += cranelift
  # LLVM could be enabled if not in Windows
  ifneq ($(OS), Windows_NT)
    # Autodetect LLVM from llvm-config
    ifneq (, $(shell which llvm-config))
      LLVM_VERSION := $(shell llvm-config --version)
      # If findstring is not empty, then it have found the value
      ifneq (, $(findstring 8,$(LLVM_VERSION))$(findstring 9,$(LLVM_VERSION)))
        backends += llvm
      endif
    else
      ifneq (, $(shell which llvm-config-8))
        backends += llvm
      endif
    endif
  endif
endif

backends := $(filter-out ,$(backends))

ifneq ($(OS), Windows_NT)
  bold := $(shell tput bold)
  green := $(shell tput setaf 2)
  reset := $(shell tput sgr0)
endif


$(info Available backends: $(bold)$(green)${backends}$(reset))

backend_features_spaced := $(foreach backend,$(backends),backend-$(backend))
backend_features := --features "$(backend_features_spaced)"

# $(info Cargo features ${backend_features})

# Generate files
generate-emtests:
	WASM_EMSCRIPTEN_GENERATE_EMTESTS=1 cargo build --release \
	&& echo "formatting" \
	&& cargo fmt

# To generate WASI tests you'll need to have the correct versions of the Rust nightly
# toolchain installed, see `WasiVersion::get_compiler_toolchain` in
# `tests/generate-wasi-tests/src/wasi_version.rs`
#
# or run `make wasitests-setup-toolchain` or `make wasitests-setup-toolchain-all`
generate-wasitests: wasitests-setup
	WASM_WASI_GENERATE_WASITESTS=1 cargo build --release -vv \
	&& echo "formatting" \
	&& cargo fmt

generate-wasitests-all: wasitests-setup
	WASI_TEST_GENERATE_ALL=1 WASM_WASI_GENERATE_WASITESTS=1 cargo build --release -vv \
	&& echo "formatting" \
	&& cargo fmt

emtests-generate: generate-emtests
wasitests-generate: generate-wasitests

wasitests-setup-toolchain: wasitests-setup
	WASM_WASI_SET_UP_TOOLCHAIN=1 cargo build --release -vv

wasitests-setup-toolchain-all: wasitests-setup
	WASI_TEST_GENERATE_ALL=1 WASM_WASI_SET_UP_TOOLCHAIN=1 cargo build --release -vv

generate: generate-emtests generate-wasitests


# Spectests
spectests-singlepass:
	cargo test singlepass::spec --release $(backend_features)

spectests-cranelift:
	cargo test cranelift::spec --release $(backend_features)

spectests-llvm:
	cargo test llvm::spec --release $(backend_features) -- --test-threads=1

spectests:
	cargo test spec --release $(backend_features) -- --test-threads=1


# Emscripten tests
emtests-singlepass:
	cargo test singlepass::emscripten --release $(backend_features)

emtests-cranelift:
	cargo test cranelift::emscripten --release $(backend_features)

emtests-llvm:
	cargo test llvm::emscripten --release $(backend_features) -- --test-threads=1

emtests-all:
	cargo test emscripten --release $(backend_features) -- --test-threads=1

emtests: emtests-singlepass emtests-cranelift emtests-llvm


# Middleware tests
middleware-singlepass:
	cargo test singlepass::middleware --release $(backend_features)

middleware-cranelift:
	cargo test cranelift::middleware --release $(backend_features)

middleware-llvm:
	cargo test llvm::middleware --release $(backend_features)

middleware: middleware-singlepass middleware-cranelift middleware-llvm


# Wasitests
wasitests-setup:
ifeq (,$(wildcard ./tests/wasi_test_resources/test_fs/temp))
	rm -rf tests/wasi_test_resources/test_fs/temp
endif
	mkdir -p tests/wasi_test_resources/test_fs/temp

wasitests-singlepass: wasitests-setup
	cargo test singlepass::wasi --release $(backend_features)

wasitests-cranelift: wasitests-setup
	cargo test cranelift::wasi --release $(backend_features) -- --test-threads=1

wasitests-llvm: wasitests-setup
	cargo test llvm::wasi --release $(backend_features) -- --test-threads=1

wasitests-all: wasitests-setup
	cargo test wasi --release $(backend_features) -- --test-threads=1

wasitests-unit: wasitests-setup
	cargo test --manifest-path lib/wasi/Cargo.toml --release

wasitests: wasitests-unit wasitests-singlepass wasitests-cranelift wasitests-llvm


# Backends
singlepass: wasitests-setup
	cargo test -p wasmer-singlepass-backend --release
	cargo test singlepass:: --release $(backend_features) -- --test-threads=1

cranelift: wasitests-setup
	cargo test -p wasmer-clif-backend --release
	cargo test cranelift:: --release $(backend_features)

llvm: wasitests-setup
	cargo test -p wasmer-llvm-backend --release
	cargo test llvm:: --release $(backend_features) -- --test-threads=1


# All tests
capi-singlepass:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features singlepass-backend,wasi

capi-cranelift:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features cranelift-backend,wasi

capi-llvm:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features llvm-backend,wasi

capi-emscripten:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features singlepass-backend,emscripten

# We use cranelift as the default backend for the capi for now
capi: capi-cranelift

test-capi-singlepass: capi-singlepass
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features singlepass-backend,wasi

test-capi-cranelift: capi-cranelift
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features cranelift-backend,wasi

test-capi-llvm: capi-llvm
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features llvm-backend,wasi

test-capi-emscripten: capi-emscripten
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml -Z unstable-options --profile release-capi \
		--no-default-features --features singlepass-backend,emscripten

test-capi: test-capi-singlepass test-capi-cranelift test-capi-llvm test-capi-emscripten

capi-test: test-capi

test-rest:
	cargo test --release -p wasmer-interface-types
	cargo test --release -p wasmer-runtime
	cargo test --release -p wasmer-runtime-core
	cargo test --release -p wasmer-wasi-experimental-io-devices
	cargo test --release -p wasmer-win-exception-handler
	# This doesn't work in windows, commented for now
	# cargo test --release -p wasmer-kernel-loader
	# cargo test --release -p kernel-net

test: $(backends) test-rest examples

test-android:
	ci/run-docker.sh x86_64-linux-android --manifest-path=lib/singlepass-backend/Cargo.toml
	ci/run-docker.sh x86_64-linux-android runtime_core

# Integration tests
integration-tests: release-clif examples
	echo "Running Integration Tests"
	./tests/integration_tests/lua/test.sh
	./tests/integration_tests/nginx/test.sh
	./tests/integration_tests/cowsay/test.sh

examples:
	cargo build --release $(backend_features) --examples
	test -f target/release/examples/callback && ./target/release/examples/callback || echo "skipping callback test"
	test -f target/release/examples/plugin && ./target/release/examples/plugin || echo "skipping plugin test"

# Utils
lint:
	cargo fmt --all -- --check

precommit: lint test

debug:
	cargo build --release --features "debug trace"

install:
	cargo install --path .

# Checks
check-bench-singlepass:
	cargo check --benches --all singlepass \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
check-bench-clif:
	cargo check --benches --all cranelift \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
check-bench-llvm:
	cargo check --benches --all llvm \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

check-bench: check-bench-singlepass check-bench-llvm

check-kernel-net:
	cargo check -p kernel-net --target=wasm32-wasi

# checks that require a nightly version of Rust
check-nightly: check-kernel-net

# TODO: We wanted `--workspace --exclude wasmer-runtime`, but can't due
# to https://github.com/rust-lang/cargo/issues/6745 .
NOT_RUNTIME_CRATES = -p wasmer-clif-backend -p wasmer-singlepass-backend -p wasmer-middleware-common -p wasmer-runtime-core -p wasmer-emscripten -p wasmer-llvm-backend -p wasmer-wasi -p wasmer-kernel-loader -p wasmer-interface-types
RUNTIME_CHECK = cargo check --manifest-path lib/runtime/Cargo.toml --no-default-features
check: check-bench
	cargo check $(NOT_RUNTIME_CRATES)
	cargo check --release $(NOT_RUNTIME_CRATES)
	cargo check --all-features $(NOT_RUNTIME_CRATES)
	cargo check --release --all-features $(NOT_RUNTIME_CRATES)
	# wasmer-runtime doesn't work with all backends enabled at once.
	#
	# We test using manifest-path directly so as to disable the default.
	# `--no-default-features` only disables the default features in the
	# current package, not the package specified by `-p`. This is
	# intentional.
	#
	# Test default features, test 'debug' feature only in non-release
	# builds, test as many combined features as possible with each backend
	# as default, and test a minimal set of features with only one backend
	# at a time.
	cargo check --manifest-path lib/runtime-core/Cargo.toml
	cargo check --manifest-path lib/runtime/Cargo.toml
	# Check some of the cases where deterministic execution could matter
	cargo check --manifest-path lib/runtime/Cargo.toml --features "deterministic-execution"
	cargo check --manifest-path lib/runtime/Cargo.toml --no-default-features \
		--features=default-backend-singlepass,deterministic-execution
	cargo check --manifest-path lib/runtime/Cargo.toml --no-default-features \
		--features=default-backend-llvm,deterministic-execution
	cargo check --release --manifest-path lib/runtime/Cargo.toml

	$(RUNTIME_CHECK) \
		--features=cranelift,cache,llvm,singlepass,default-backend-singlepass
	$(RUNTIME_CHECK) --release \
		--features=cranelift,cache,llvm,singlepass,default-backend-singlepass
	$(RUNTIME_CHECK) \
		--features=cranelift,cache,llvm,singlepass,default-backend-cranelift
	$(RUNTIME_CHECK) --release \
		--features=cranelift,cache,llvm,singlepass,default-backend-cranelift
	$(RUNTIME_CHECK) \
		--features=cranelift,cache,llvm,singlepass,default-backend-llvm
	$(RUNTIME_CHECK) --release \
		--features=cranelift,cache,llvm,singlepass,default-backend-llvm
	$(RUNTIME_CHECK) \
		--features=singlepass,default-backend-singlepass
	$(RUNTIME_CHECK) --release \
		--features=singlepass,default-backend-singlepass
	$(RUNTIME_CHECK) \
		--features=cranelift,default-backend-cranelift
	$(RUNTIME_CHECK) --release \
		--features=cranelift,default-backend-cranelift
	$(RUNTIME_CHECK) \
		--features=llvm,default-backend-llvm
	$(RUNTIME_CHECK) --release \
		--features=llvm,default-backend-llvm
		--features=default-backend-singlepass,singlepass,cranelift,llvm,cache,deterministic-execution

# Release
release:
	cargo build --release $(backend_features) --features experimental-io-devices,log/release_max_level_off

# Release with musl target
release-musl:
	# backend-llvm is not included due to dependency on wabt.
	# experimental-io-devices is not included due to missing x11-fb.
	cargo build --release --target x86_64-unknown-linux-musl --features backend-singlepass,backend-cranelift,loader-kernel,log/release_max_level_off,wasi --no-default-features

# This way of releasing is deprecated, since backends are now detected
# automatically
release-clif: release

# This way of releasing is deprecated, since backends are now detected
# automatically
release-singlepass: release

# This way of releasing is deprecated, since backends are now detected
# automatically
release-llvm: release

bench-singlepass:
# NOTE this will run some benchmarks using clif; TODO: fix this
	cargo bench --all singlepass \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
bench-clif:
	cargo bench --all cranelift \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
bench-llvm:
# NOTE this will run some benchmarks using clif; TODO: fix this
	cargo bench --all llvm \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

build-install-package:
	# This command doesn't build the binary, just packages it
	mkdir -p ./install/bin
	cp ./wapm-cli/target/release/wapm ./install/bin/
	cp ./target/release/wasmer ./install/bin/
	# Create the wax binary as symlink to wapm
	cd ./install/bin/ && ln -sf wapm wax && chmod +x wax
	tar -C ./install -zcvf wasmer.tar.gz bin

build-capi-package:
	# This command doesn't build the C-API, just packages it
	mkdir -p ./capi/
	mkdir -p ./capi/include
	mkdir -p ./capi/lib
ifeq ($(OS), Windows_NT)
	cp target/release/wasmer_runtime_c_api.dll ./capi/lib/wasmer.dll
	cp target/release/wasmer_runtime_c_api.lib ./capi/lib/wasmer.lib
else
ifeq ($(UNAME_S), Darwin)
	cp target/release/libwasmer_runtime_c_api.dylib ./capi/lib/libwasmer.dylib
	cp target/release/libwasmer_runtime_c_api.a ./capi/lib/libwasmer.a
	# Fix the rpath for the dylib
	install_name_tool -id "@rpath/libwasmer.dylib" ./capi/lib/libwasmer.dylib
else
	cp target/release/libwasmer_runtime_c_api.so ./capi/lib/libwasmer.so
	cp target/release/libwasmer_runtime_c_api.a ./capi/lib/libwasmer.a
endif
endif
	find target/release/build -name 'wasmer.h*' -exec cp {} ./capi/include ';'
	cp LICENSE ./capi/LICENSE
	cp lib/runtime-c-api/doc/index.md ./capi/README.md
	tar -C ./capi -zcvf wasmer-c-api.tar.gz lib include README.md LICENSE

WAPM_VERSION = v0.5.0
build-wapm:
	git clone --branch $(WAPM_VERSION) https://github.com/wasmerio/wapm-cli.git
	cargo build --release --manifest-path wapm-cli/Cargo.toml --features "telemetry update-notifications"

# For installing the contents locally
do-install:
	tar -C ~/.wasmer -zxvf wasmer.tar.gz

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/

# cargo install cargo-deps
# must install graphviz for `dot`
dep-graph:
	cargo deps --optional-deps --filter wasmer-wasi wasmer-kernel-loader wasmer-llvm-backend wasmer-emscripten wasmer-runtime-core wasmer-runtime wasmer-middleware-common wasmer-singlepass-backend wasmer-clif-backend wasmer --manifest-path Cargo.toml | dot -Tpng > wasmer_depgraph.png

docs-capi:
	cd lib/runtime-c-api/ && doxygen doxyfile

docs: docs-capi
	cargo doc --release --features=backend-singlepass,backend-cranelift,backend-llvm,docs,wasi,managed --workspace --document-private-items --no-deps
	mkdir -p api-docs
	mkdir -p api-docs/c
	cp -R target/doc api-docs/crates
	cp -R lib/runtime-c-api/doc/html api-docs/c/runtime-c-api
	echo '<!-- Build $(SOURCE_VERSION) --><meta http-equiv="refresh" content="0; url=rust/wasmer_runtime/index.html">' > api-docs/index.html
	echo '<!-- Build $(SOURCE_VERSION) --><meta http-equiv="refresh" content="0; url=wasmer_runtime/index.html">' > api-docs/crates/index.html

docs-publish:
	git clone -b "gh-pages" --depth=1 https://wasmerbot:$(GITHUB_DOCS_TOKEN)@github.com/wasmerio/wasmer.git api-docs-repo
	cp -R api-docs/* api-docs-repo/
	cd api-docs-repo && git add index.html crates/* c/*
	cd api-docs-repo && (git diff-index --quiet HEAD || git commit -m "Publishing GitHub Pages")
	cd api-docs-repo && git push origin gh-pages
