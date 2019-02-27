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

capi:
	WASM_EMSCRIPTEN_GENERATE_C_API_HEADERS=1 cargo build --manifest-path lib/runtime-c-api/Cargo.toml --features generate-c-api-headers

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
	cargo clippy --all

precommit: lint test

test:
	# We use one thread so the emscripten stdouts doesn't collide
	cargo test --all --exclude wasmer-runtime-c-api -- --test-threads=1 $(runargs)
	# cargo test --all --exclude wasmer-emscripten -- --test-threads=1 $(runargs)
	cargo build -p wasmer-runtime-c-api
	cargo test -p wasmer-runtime-c-api -- --nocapture

release:
	# If you are in OS-X, you will need mingw-w64 for cross compiling to windows
	# brew install mingw-w64
	cargo build --release

debug-release:
	cargo build --release --features debug

debug-release:
	cargo build --release --features "debug"

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/
