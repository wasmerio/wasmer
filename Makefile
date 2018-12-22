ifeq (test, $(firstword $(MAKECMDGOALS)))
  runargs := $(wordlist 2, $(words $(MAKECMDGOALS)), $(MAKECMDGOALS))
  $(eval $(runargs):;@true)
endif

.PHONY: spectests emtests clean build install lint precommit

# This will re-generate the Rust test files based on spectests/*.wast
spectests:
	WASM_GENERATE_SPECTESTS=1 cargo build

emtests:
	WASM_GENERATE_EMTESTS=1 cargo build

# clean:
#     rm -rf artifacts

build:
	cargo build

install:
	cargo install --path .

lint:
	cargo fmt -- --check

precommit: lint test

test:
	# We use one thread so the emscripten stdouts doesn't collide
	cargo test -- --test-threads=1 $(runargs) 

release:
	# If you are in OS-X, you will need mingw-w64 for cross compiling to windows
	# brew install mingw-w64
	cargo build --release

debug-release:
	cargo build --release --features "debug"

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/
