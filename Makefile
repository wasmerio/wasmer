.PHONY: spectests emtests clean build install lint precommit docs examples

ARCH := $(shell uname -m)

ifeq ($(ARCH), x86_64)
  # In X64, all backends are enabled
  ifeq ($(OS), Windows_NT)
    # llvm tests are failing in Windows
    backends := cranelift
  else
    backends := singlepass cranelift llvm
  endif
  default_backend := cranelift
else ifeq ($(ARCH), aarch64)
  # In ARM 64, only singlepass is enabled
  backends := singlepass
  default_backend := singlepass
else
  $(error Architecture $(ARCH) not yet supported in Wasmer)
endif

# We convert `$backend1 $backend2 ...` to `backend-$backend1,backend-$backend2,..."
comma := ,
empty :=
space := $(empty) $(empty)
backend_features_spaced := $(foreach backend,$(backends),backend-$(backend))
backend_features:= $(subst $(space),$(comma),$(backend_features_spaced))

# Generate files
generate-spectests:
	WASMER_RUNTIME_GENERATE_SPECTESTS=1 cargo build -p wasmer-runtime-core --release \
	&& echo "formatting" \
	&& cargo fmt

generate-emtests:
	WASM_EMSCRIPTEN_GENERATE_EMTESTS=1 cargo build -p wasmer-emscripten-tests --release \
	&& echo "formatting" \
	&& cargo fmt

generate-wasitests: wasitests-setup
	WASM_WASI_GENERATE_WASITESTS=1 cargo build -p wasmer-wasi-tests --release -vv \
	&& echo "formatting" \
	&& cargo fmt

spectests-generate: generate-spectests
emtests-generate: generate-emtests
wasitests-generate: generate-wasitests

generate: generate-spectests generate-emtests generate-wasitests


# Spectests
spectests-singlepass:
	cargo test -p wasmer-spectests --release --features default-backend-singlepass -- --nocapture --test-threads 1

spectests-cranelift:
	cargo test -p wasmer-spectests --release --features default-backend-cranelift -- --nocapture

spectests-llvm:
	cargo test -p wasmer-spectests --release --features default-backend-llvm -- --nocapture

spectests: $(foreach backend,$(backends),spectests-$(backend))


# Emscripten tests
emtests-singlepass:
	cargo test -p wasmer-emscripten-tests --release --features default-backend-singlepass -- --test-threads=1

emtests-cranelift:
	cargo test -p wasmer-emscripten-tests --release --features default-backend-cranelift -- --test-threads=1

emtests-llvm:
	cargo test -p wasmer-emscripten-tests --release --features default-backend-llvm -- --test-threads=1

emtests-unit:
	cargo test -p wasmer-emscripten --release

emtests: emtests-unit $(foreach backend,$(backends),emtests-$(backend))


# Middleware tests
middleware-singlepass:
	cargo test -p wasmer-middleware-common --release --features default-backend-singlepass

middleware-cranelift:
	cargo test -p wasmer-middleware-common --release --features default-backend-cranelift

middleware-llvm:
	cargo test -p wasmer-middleware-common --release --features default-backend-llvm

middleware: $(foreach backend,$(backends),middleware-$(backend))


# Wasitests
wasitests-setup:
	rm -rf lib/wasi-tests/wasitests/test_fs/temp
	mkdir -p lib/wasi-tests/wasitests/test_fs/temp

wasitests-singlepass: wasitests-setup
	# Skip wasitests on singlepass for now, until they are fixed on ARM devices
	# cargo test -p wasmer-wasi-tests --release --features default-backend-singlepass -- --test-threads=1 --nocapture

wasitests-cranelift: wasitests-setup
	cargo test -p wasmer-wasi-tests --release --features default-backend-cranelift -- --test-threads=1 --nocapture

wasitests-llvm: wasitests-setup
	cargo test -p wasmer-wasi-tests --release --features default-backend-llvm -- --test-threads=1 --nocapture

wasitests-unit: wasitests-setup
	cargo test -p wasmer-wasi --release

wasitests: wasitests-unit $(foreach backend,$(backends),wasitests-$(backend))


# Backends
singlepass: spectests-singlepass emtests-singlepass middleware-singlepass wasitests-singlepass
	cargo test -p wasmer-singlepass-backend --release
	cargo test -p wasmer-runtime-core-tests --release --no-default-features --features backend-singlepass

cranelift: spectests-cranelift emtests-cranelift middleware-cranelift wasitests-cranelift
	cargo test -p wasmer-clif-backend --release
	cargo test -p wasmer-runtime-core-tests --release

llvm: spectests-llvm emtests-llvm wasitests-llvm
	cargo test -p wasmer-llvm-backend --release
	cargo test -p wasmer-llvm-backend-tests --release
	cargo test -p wasmer-runtime-core-tests --release --no-default-features --features backend-llvm


# All tests
capi-singlepass:
	cargo build -p wasmer-runtime-c-api --release \
		--no-default-features --features default-backend-singlepass,wasi,emscripten

capi-cranelift:
	cargo build -p wasmer-runtime-c-api --release \
		--no-default-features --features default-backend-cranelift,wasi,emscripten

capi-llvm:
	cargo build -p wasmer-runtime-c-api --release \
		--no-default-features --features default-backend-llvm,wasi,emscripten

capi: capi-${default_backend}


test-capi-singlepass: capi-singlepass
	cargo test -p wasmer-runtime-c-api --release \
		--no-default-features --features default-backend-singlepass,wasi,emscripten

test-capi-cranelift: capi-cranelift
	cargo test -p wasmer-runtime-c-api --release \
		--no-default-features --features default-backend-cranelift,wasi,emscripten

test-capi-llvm: capi-llvm
	cargo test -p wasmer-runtime-c-api --release \
		--no-default-features --features default-backend-llvm,wasi,emscripten

test-capi: $(foreach backend,$(backends),test-capi-$(backend))

capi-test: test-capi

# Runtimes
runtime-singlepass:
	cargo test --manifest-path lib/runtime/Cargo.toml --release --no-default-features --features default-backend-singlepass

runtime-cranelift:
	cargo test --manifest-path lib/runtime/Cargo.toml --release --no-default-features --features default-backend-cranelift

runtime-llvm:
	cargo test --manifest-path lib/runtime/Cargo.toml --release --no-default-features --features default-backend-llvm

runtime: $(foreach backend,$(backends),runtime-$(backend))

# The rest
test-rest:
	cargo test --release --features backend-${default_backend} \
		--workspace \
		--exclude wasmer-runtime-c-api \
		--exclude wasmer-runtime \
		--exclude wasmer-emscripten \
		--exclude wasmer-spectests \
		--exclude wasmer-wasi \
		--exclude wasmer-middleware-common \
		--exclude wasmer-middleware-common-tests \
		--exclude wasmer-singlepass-backend \
		--exclude wasmer-clif-backend \
		--exclude wasmer-llvm-backend \
		--exclude wasmer-wasi-tests \
		--exclude wasmer-emscripten-tests \
		--exclude wasmer-runtime-core-tests

tests: spectests emtests middleware wasitests test-rest examples

test: tests

# Integration tests
integration-tests: release examples
	echo "Running Integration Tests"
	./integration_tests/lua/test.sh
	./integration_tests/nginx/test.sh
	./integration_tests/cowsay/test.sh

examples:
	cargo run --example plugin --features default-backend-${default_backend}
	cargo run --example callback --features default-backend-${default_backend}


# Utils
lint:
	cargo fmt --all -- --check

precommit: lint test

debug:
	cargo build --release --features backend-${default_backend},debug,trace

install:
	cargo install --path .

# Checks
check-bench-singlepass:
	cargo check --benches --all --no-default-features --features backend-singlepass \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader

check-bench-cranelift:
	cargo check --benches --all --no-default-features --features backend-cranelift \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader \
	--exclude wasmer-middleware-common-tests

check-bench-llvm:
	cargo check --benches --all --no-default-features --features backend-llvm \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

check-bench: $(foreach backend,$(backends),check-bench-$(backend))


# TODO: We wanted `--workspace --exclude wasmer-runtime`, but can't due
# to https://github.com/rust-lang/cargo/issues/6745 .
NOT_RUNTIME_CRATES = -p wasmer-clif-backend -p wasmer-singlepass-backend -p wasmer-middleware-common -p wasmer-runtime-core -p wasmer-emscripten -p wasmer-llvm-backend -p wasmer-wasi -p wasmer-kernel-loader -p wasmer-dev-utils -p wasmer-wasi-tests -p wasmer-middleware-common-tests -p wasmer-emscripten-tests
RUNTIME_CHECK = cargo check -p wasmer-runtime --no-default-features
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
	cargo check -p wasmer-runtime

	$(RUNTIME_CHECK) \
		--features=default-backend-${default_backend},${backend_features},cache,debug,deterministic-execution


# Release
release:
	cargo build --release --features default-backend-${default_backend},${backend_features}

release-cranelift:
	cargo build --release --features default-backend-cranelift

release-singlepass:
	cargo build --release --features default-backend-singlepass

release-llvm:
	cargo build --release --features default-backend-llvm


# Benchmarks
bench-singlepass:
	cargo bench --all --no-default-features --features default-backend-singlepass \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader

bench-cranelift:
	cargo bench --all --no-default-features --features default-backend-cranelift \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader \
	--exclude wasmer-middleware-common-tests

bench-llvm:
	cargo bench --all --no-default-features --features default-backend-llvm \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

bench: $(foreach backend,$(backends),bench-$(backend))


# Build utils
build-install:
	mkdir -p ./install/bin
	cp ./wapm-cli/target/release/wapm ./install/bin/
	cp ./target/release/wasmer ./install/bin/
	tar -C ./install -zcvf wasmer.tar.gz bin/wapm bin/wasmer

# For installing the contents locally
do-install:
	tar -C ~/.wasmer -zxvf wasmer.tar.gz

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/

# cargo install cargo-deps
# must install graphviz for `dot`
dep-graph:
	cargo deps --optional-deps --filter wasmer-wasi wasmer-wasi-tests wasmer-kernel-loader wasmer-dev-utils wasmer-llvm-backend wasmer-emscripten wasmer-emscripten-tests wasmer-runtime-core wasmer-runtime wasmer-middleware-common wasmer-middleware-common-tests wasmer-singlepass-backend wasmer-clif-backend wasmer --manifest-path Cargo.toml | dot -Tpng > wasmer_depgraph.png

docs:
	cargo doc --features=backend-singlepass,backend-cranelift,backend-llvm,default-backend-${default_backend},docs,wasi,managed

wapm:
	cargo build --release --manifest-path wapm-cli/Cargo.toml --features telemetry,update-notifications
