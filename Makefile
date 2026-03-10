.PHONY: spectests emtests clean build install lint precommit docs examples

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
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features singlepass -- --nocapture --test-threads 1

spectests-cranelift:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features clif -- --nocapture

spectests-llvm:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features llvm -- --nocapture

spectests: spectests-singlepass spectests-cranelift spectests-llvm


# Emscripten tests
emtests-singlepass:
	cargo test --manifest-path lib/emscripten-tests/Cargo.toml --release --features singlepass -- --test-threads=1

emtests-cranelift:
	cargo test --manifest-path lib/emscripten-tests/Cargo.toml --release --features clif -- --test-threads=1

emtests-llvm:
	cargo test --manifest-path lib/emscripten-tests/Cargo.toml --release --features llvm -- --test-threads=1

emtests-unit:
	cargo test --manifest-path lib/emscripten/Cargo.toml --release

emtests: emtests-unit emtests-singlepass emtests-cranelift emtests-llvm


# Middleware tests
middleware-singlepass:
	cargo test --manifest-path lib/middleware-common-tests/Cargo.toml --release --features singlepass

middleware-cranelift:
	cargo test --manifest-path lib/middleware-common-tests/Cargo.toml --release --features clif

middleware-llvm:
	cargo test --manifest-path lib/middleware-common-tests/Cargo.toml --release --features llvm

middleware: middleware-singlepass middleware-cranelift middleware-llvm


# Wasitests
wasitests-setup:
	rm -rf lib/wasi-tests/wasitests/test_fs/temp
	mkdir -p lib/wasi-tests/wasitests/test_fs/temp

wasitests-singlepass: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features singlepass -- --test-threads=1

wasitests-cranelift: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features clif -- --test-threads=1 --nocapture

wasitests-llvm: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features llvm -- --test-threads=1

wasitests-unit: wasitests-setup
	cargo test --manifest-path lib/wasi-tests/Cargo.toml --release --features clif -- --test-threads=1 --nocapture
	cargo test --manifest-path lib/wasi/Cargo.toml --release

wasitests: wasitests-unit wasitests-singlepass wasitests-cranelift wasitests-llvm


# Backends
singlepass: spectests-singlepass emtests-singlepass middleware-singlepass wasitests-singlepass
	cargo test -p wasmer-singlepass-backend --release
	cargo test --manifest-path lib/runtime-core-tests/Cargo.toml --release --no-default-features --features backend-singlepass

cranelift: spectests-cranelift emtests-cranelift middleware-cranelift wasitests-cranelift
	cargo test -p wasmer-clif-backend --release
	cargo test -p wasmer-runtime-core-tests --release

llvm: spectests-llvm emtests-llvm wasitests-llvm
	cargo test -p wasmer-llvm-backend --release
	cargo test -p wasmer-llvm-backend-tests --release
	cargo test --manifest-path lib/runtime-core-tests/Cargo.toml --release --no-default-features --features backend-llvm


# All tests
capi:
	cargo build -p wasmer-runtime-c-api --release

capi-linux-amd64: capi
	mv target/release/libwasmer_runtime_c_api.so target/release/libwasmer_linux_amd64.so
	patchelf --set-soname libwasmer_linux_amd64.so target/release/libwasmer_linux_amd64.so

capi-linux-arm64: capi
	mv target/release/libwasmer_runtime_c_api.so target/release/libwasmer_linux_arm64.so
	patchelf --set-soname libwasmer_linux_arm64.so target/release/libwasmer_linux_arm64.so

capi-osx-amd64: capi
	mv target/release/libwasmer_runtime_c_api.dylib target/release/libwasmer_darwin_amd64.dylib
	install_name_tool -id @executable_path/libwasmer_darwin_amd64.dylib target/release/libwasmer_darwin_amd64.dylib;

capi-singlepass:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,wasi

capi-cranelift:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features cranelift-backend,wasi

capi-llvm:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features llvm-backend,wasi

capi-emscripten:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,emscripten

test-capi-singlepass: capi-singlepass
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,wasi

capi-dev:
	cargo build -p wasmer-runtime-c-api --profile dev

test-capi: capi
	cargo test -p wasmer-runtime-c-api --release
test-capi-cranelift: capi-cranelift
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features cranelift-backend,wasi

test-capi-llvm: capi-llvm
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features llvm-backend,wasi

test-capi-emscripten: capi-emscripten
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,emscripten

test-capi: test-capi-singlepass

capi-test: test-capi

test-rest:
	cargo test --release -p wasmer-dev-utils
	cargo test --release -p wasmer-interface-types
	cargo test --release -p wasmer-kernel-loader
	cargo test --release -p kernel-net
	cargo test --release -p wasmer-llvm-backend-tests
	cargo test --release -p wasmer-runtime
	cargo test --release -p wasmer-runtime-core
	cargo test --release -p wasmer-wasi-experimental-io-devices
	cargo test --release -p wasmer-win-exception-handler

test: spectests emtests middleware wasitests test-rest examples


# Integration tests
integration-tests: release-clif examples
	echo "Running Integration Tests"
	./integration_tests/lua/test.sh
	./integration_tests/nginx/test.sh
	./integration_tests/cowsay/test.sh

examples:
	cargo run --example plugin
	cargo run --example callback


# Utils
lint:
	cargo fmt --all -- --check

precommit: lint test

debug:
	cargo build --release --features backend-cranelift,backend-singlepass,debug,trace

install:
	cargo install --path .

# Checks
check-bench-singlepass:
	cargo check --benches --all --no-default-features --features "backend-singlepass" \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
check-bench-clif:
	cargo check --benches --all --no-default-features --features "backend-cranelift" \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader \
	--exclude wasmer-middleware-common-tests
check-bench-llvm:
	cargo check --benches --all --no-default-features --features "backend-llvm" \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

check-bench: check-bench-singlepass check-bench-llvm

# TODO: We wanted `--workspace --exclude wasmer-runtime`, but can't due
# to https://github.com/rust-lang/cargo/issues/6745 .
NOT_RUNTIME_CRATES = -p wasmer-clif-backend -p wasmer-singlepass-backend -p wasmer-middleware-common -p wasmer-runtime-core -p wasmer-emscripten -p wasmer-llvm-backend -p wasmer-wasi -p wasmer-kernel-loader -p wasmer-dev-utils -p wasmer-wasi-tests -p wasmer-middleware-common-tests -p wasmer-emscripten-tests -p wasmer-interface-types
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
	cargo build --release --features backend-singlepass,backend-cranelift,backend-llvm,loader-kernel,experimental-io-devices,log/release_max_level_off

# Release with musl target
release-musl:
	# backend-llvm is not included due to dependency on wabt.
	# experimental-io-devices is not included due to missing x11-fb.
	cargo build --release --target x86_64-unknown-linux-musl --features backend-singlepass,backend-cranelift,loader-kernel,log/release_max_level_off,wasi --no-default-features

# Only one backend (cranelift)
release-clif:
	# If you are on macOS, you will need mingw-w64 for cross compiling to Windows
	# brew install mingw-w64
	cargo build --release --features backend-cranelift

release-singlepass:
	cargo build --release --features backend-singlepass

release-llvm:
	cargo build --release --features backend-llvm,experimental-io-devices

bench-singlepass:
	cargo bench --all --no-default-features --features "backend-singlepass" \
	--exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader
bench-clif:
	cargo bench --all --no-default-features --features "backend-cranelift" \
	--exclude wasmer-singlepass-backend --exclude wasmer-llvm-backend --exclude wasmer-kernel-loader \
	--exclude wasmer-middleware-common-tests
bench-llvm:
	cargo bench --all --no-default-features --features "backend-llvm" \
	--exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-kernel-loader

build-install-package:
	# This command doesn't build the binary, just packages it
	mkdir -p ./install/bin
	cp ./wapm-cli/target/release/wapm ./install/bin/
	cp ./target/release/wasmer ./install/bin/
	tar -C ./install -zcvf wasmer.tar.gz bin/wapm bin/wasmer

UNAME_S := $(shell uname -s)

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
	cp target/release/libwasmer_runtime_c_api.dylib ./capi/lib/libwasmer.a
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

WAPM_VERSION = 0.4.3
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
	cargo deps --optional-deps --filter wasmer-wasi wasmer-wasi-tests wasmer-kernel-loader wasmer-dev-utils wasmer-llvm-backend wasmer-emscripten wasmer-emscripten-tests wasmer-runtime-core wasmer-runtime wasmer-middleware-common wasmer-middleware-common-tests wasmer-singlepass-backend wasmer-clif-backend wasmer --manifest-path Cargo.toml | dot -Tpng > wasmer_depgraph.png

docs-capi:
	cd lib/runtime-c-api/ && doxygen doxyfile

docs: docs-capi
	cargo doc --features=backend-singlepass,backend-cranelift,backend-llvm,docs,wasi,managed --workspace --document-private-items --no-deps
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
