ifeq (test, $(firstword $(MAKECMDGOALS)))
  runargs := $(wordlist 2, $(words $(MAKECMDGOALS)), $(MAKECMDGOALS))
  $(eval $(runargs):;@true)
endif

.PHONY: spectests emtests clean build install lint precommit

# This will re-generate the Rust test files based on spectests/*.wast
spectests:
	WASMER_RUNTIME_GENERATE_SPECTESTS=1 cargo build -p wasmer-runtime

emtests:
	WASM_EMSCRIPTEN_GENERATE_EMTESTS=1 cargo build -p wasmer-emscripten

# clean:
#     rm -rf artifacts

build:
	cargo build

install:
	cargo install --path .

integration-tests: release
	echo "Running Integration Tests"
	# Commented for now until we fix emscripten
	# ./integration_tests/nginx/test.sh

lint:
	cargo fmt --all -- --check
	cargo clippy --all

precommit: lint test

test:
	# We use one thread so the emscripten stdouts doesn't collide
	# cargo test --all -- --test-threads=1 $(runargs)
	# cargo test --all --exclude wasmer-emscripten -- --test-threads=1 $(runargs)
	cargo test -p wasmer-runtime -- --test-threads=1 $(runargs)

release:
	# If you are in OS-X, you will need mingw-w64 for cross compiling to windows
	# brew install mingw-w64
	cargo build --release

debug-release:
	cargo build --release --features "debug"

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/
