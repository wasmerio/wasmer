.PHONY: spectests emtests clean build install lint precommit

# Generate files
generate-spectests:
	WASMER_RUNTIME_GENERATE_SPECTESTS=1 cargo build -p wasmer-runtime-core --release

generate-emtests:
	WASM_EMSCRIPTEN_GENERATE_EMTESTS=1 cargo build -p wasmer-emscripten --release

generate-wasitests:
	WASM_WASI_GENERATE_WASITESTS=1 cargo build -p wasmer-wasi --release

generate: generate-spectests generate-emtests generate-wasitests


# Spectests
spectests-singlepass:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features singlepass

spectests-cranelift:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features clif

spectests-llvm:
	cargo test --manifest-path lib/spectests/Cargo.toml --release --features llvm

spectests: spectests-singlepass spectests-cranelift spectests-llvm


# Emscripten tests
emtests-singlepass:
	cargo test --manifest-path lib/emscripten/Cargo.toml --release --features singlepass -- --test-threads=1

emtests-cranelift:
	cargo test --manifest-path lib/emscripten/Cargo.toml --release --features clif -- --test-threads=1

emtests-llvm:
	cargo test --manifest-path lib/emscripten/Cargo.toml --release --features llvm -- --test-threads=1

emtests: emtests-singlepass emtests-cranelift emtests-llvm


# Middleware tests
middleware-singlepass:
	cargo test --manifest-path lib/middleware-common/Cargo.toml --release --features singlepass

middleware-cranelift:
	cargo test --manifest-path lib/middleware-common/Cargo.toml --release --features clif

middleware-llvm:
	cargo test --manifest-path lib/middleware-common/Cargo.toml --release --features llvm

middleware: middleware-singlepass middleware-cranelift middleware-llvm


# Wasitests
wasitests-singlepass:
	cargo test --manifest-path lib/wasi/Cargo.toml --release --features singlepass -- --test-threads=1

wasitests-cranelift:
	cargo test --manifest-path lib/wasi/Cargo.toml --release --features clif -- --test-threads=1

wasitests-llvm:
	cargo test --manifest-path lib/wasi/Cargo.toml --release --features llvm -- --test-threads=1

wasitests: wasitests-singlepass wasitests-cranelift wasitests-llvm


# Backends
singlepass: spectests-singlepass emtests-singlepass middleware-singlepass wasitests-singlepass
	cargo test -p wasmer-singlepass-backend --release

cranelift: spectests-cranelift emtests-cranelift middleware-cranelift wasitests-cranelift
	cargo test -p wasmer-clif-backend --release

llvm: spectests-llvm emtests-llvm middleware-llvm wasitests-llvm
	cargo test -p wasmer-llvm-backend --release


# All tests
capi:
	cargo build --release
	cargo build -p wasmer-runtime-c-api --release
	cargo test -p wasmer-runtime-c-api --release

test-rest: capi
	cargo test --release --all --exclude wasmer-runtime-c-api --exclude wasmer-emscripten --exclude wasmer-spectests --exclude wasmer-wasi --exclude wasmer-middleware-common --exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-llvm-backend

circleci-clean:
	@if [ ! -z "${CIRCLE_JOB}" ]; then rm -f /home/circleci/project/target/debug/deps/libcranelift_wasm* && rm -f /Users/distiller/project/target/debug/deps/libcranelift_wasm*; fi;

test: spectests emtests middleware wasitests circleci-clean test-rest


# Integration tests
integration-tests: release-fast
	echo "Running Integration Tests"
	./integration_tests/lua/test.sh
	./integration_tests/nginx/test.sh
	./integration_tests/cowsay/test.sh


# Utils
lint:
	cargo fmt --all -- --check

precommit: lint test

build:
	cargo build --release --features debug

install:
	cargo install --path .

release:
	cargo build --release --features backend:singlepass,backend:llvm,loader:kernel

# Only one backend (cranelift)
release-fast:
	# If you are in OS-X, you will need mingw-w64 for cross compiling to windows
	# brew install mingw-w64
	cargo build --release

bench:
	cargo bench --all


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
	cargo deps --optional-deps --filter wasmer-wasi wasmer-kernel-loader wasmer-dev-utils wasmer-llvm-backend wasmer-emscripten wasmer-runtime-core wasmer-runtime wasmer-middleware-common wasmer-singlepass-backend wasmer-clif-backend wasmer --manifest-path Cargo.toml | dot -Tpng > wasmer_depgraph.png
