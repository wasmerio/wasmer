.PHONY: spectests emtests clean build install lint precommit docs

# Generate files
generate-spectests:
	WASMER_RUNTIME_GENERATE_SPECTESTS=1 cargo build -p wasmer-runtime-core --release

generate-emtests:
	WASM_EMSCRIPTEN_GENERATE_EMTESTS=1 cargo build -p wasmer-emscripten-tests --release

generate-wasitests: wasitests-setup
	WASM_WASI_GENERATE_WASITESTS=1 cargo build -p wasmer-wasi-tests --release -vv \
	&& echo "formatting" \
	&& cargo fmt

spectests-generate: generate-spectests
emtests-generate: generate-emtests
wasitests-generate: generate-wasitests

spectests-cranelift: generate-spectests
	echo removed

generate: generate-spectests generate-emtests generate-wasitests


# Spectests
spectests-llvm: generate-spectests
	RUST_BACKTRACE=1 cargo test --manifest-path lib/spectests/Cargo.toml --release --features llvm -- --nocapture

spectests: spectests-llvm


# Backends
llvm: spectests-llvm
	cargo test -p wasmer-llvm-backend --release


# All tests
capi:
	cargo build --release
	cargo build -p wasmer-runtime-c-api --release

test-capi: capi
	cargo test -p wasmer-runtime-c-api --release

test-rest:
	cargo test --release --all --exclude wasmer-runtime-c-api --exclude wasmer-emscripten --exclude wasmer-spectests --exclude wasmer-wasi --exclude wasmer-middleware-common --exclude wasmer-middleware-common-tests --exclude wasmer-singlepass-backend --exclude wasmer-clif-backend --exclude wasmer-llvm-backend --exclude wasmer-wasi-tests --exclude wasmer-emscripten-tests

circleci-clean:
	@if [ ! -z "${CIRCLE_JOB}" ]; then rm -f /home/circleci/project/target/debug/deps/libcranelift_wasm* && rm -f /Users/distiller/project/target/debug/deps/libcranelift_wasm*; fi;

test: spectests circleci-clean


# Utils
lint:
	cargo fmt --all -- --check

precommit: lint test

debug:
	cargo build --release --features backend-singlepass,debug,trace

install:
	cargo install --path .

# Checks
check:
	cargo check --release --features backend-llvm

# Release
release:
	cargo build --release --features backend-llvm

release-llvm:
	cargo build --release --features backend-llvm

bench-llvm:
	cargo bench --all --no-default-features --features "backend-llvm"

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
	cargo doc --features=backend-singlepass,backend-llvm,wasi,managed
