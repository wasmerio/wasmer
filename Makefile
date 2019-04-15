ifeq (test, $(firstword $(MAKECMDGOALS)))
  runargs := $(wordlist 2, $(words $(MAKECMDGOALS)), $(MAKECMDGOALS))
  $(eval $(runargs):;@true)
endif

.PHONY: spectests emtests clean build install lint precommit

# This will re-generate the Rust test files based on spectests/*.wast
spectests:
	WASMER_RUNTIME_GENERATE_SPECTESTS=1 cargo build -p wasmer-runtime-core

emtests:
	WASM_EMSCRIPTEN_GENERATE_EMTESTS=1 cargo build -p wasmer-emscripten

# clean:
#     rm -rf artifacts

build:
	cargo build --features debug

install:
	cargo install --path .

integration-tests: release
	echo "Running Integration Tests"
	./integration_tests/lua/test.sh
	./integration_tests/nginx/test.sh

lint:
	cargo fmt --all -- --check
	cargo +nightly-2019-02-27 clippy --all

precommit: lint test

build-install:
	mkdir -p ./install/bin
	cp ./target/release/wasmer ./install/bin/
	tar -C ./install -zcvf wasmer.tar.gz bin/wasmer

# For installing the contents locally
do-install:
	tar -C ~/.wasmer -zxvf wasmer.tar.gz

test:
	# We use one thread so the emscripten stdouts doesn't collide
	cargo test --all --exclude wasmer-runtime-c-api --exclude wasmer-emscripten --exclude wasmer-spectests --exclude wasmer-singlepass-backend -- $(runargs)
	# cargo test --all --exclude wasmer-emscripten -- --test-threads=1 $(runargs)
	cargo test --manifest-path lib/spectests/Cargo.toml --features clif
	cargo test --manifest-path lib/spectests/Cargo.toml --features llvm
	cargo build -p wasmer-runtime-c-api
	cargo test -p wasmer-runtime-c-api -- --nocapture

test-singlepass:
	cargo test --manifest-path lib/spectests/Cargo.toml --features singlepass

test-emscripten-llvm:
	cargo test --manifest-path lib/emscripten/Cargo.toml --features llvm -- --test-threads=1 $(runargs)

test-emscripten-clif:
	cargo test --manifest-path lib/emscripten/Cargo.toml --features clif -- --test-threads=1 $(runargs)

test-emscripten-singlepass:
	cargo test --manifest-path lib/emscripten/Cargo.toml --features singlepass -- --test-threads=1 $(runargs)

singlepass-debug-release:
	cargo +nightly build --features "singlepass debug" --release

singlepass-release:
	cargo +nightly build --features "singlepass" --release

singlepass-build:
	cargo +nightly build --features "singlepass debug"

release:
	# If you are in OS-X, you will need mingw-w64 for cross compiling to windows
	# brew install mingw-w64
	cargo build --release

production-release:
	cargo build --release --features backend:singlepass,backend:llvm

debug-release:
	cargo build --release --features "debug"

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/
