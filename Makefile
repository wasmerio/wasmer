ifeq (test, $(firstword $(MAKECMDGOALS)))
  runargs := $(wordlist 2, $(words $(MAKECMDGOALS)), $(MAKECMDGOALS))
  $(eval $(runargs):;@true)
endif

.PHONY: spectests clean build install

# This will re-generate the Rust test files based on spectests/*.wast
spectests:
	WASM_GENERATE_SPECTESTS=1 cargo build

# clean:
#     rm -rf artifacts

build:
	cargo build

install:
	cargo install --path .

test:
	cargo test -- --test-threads=1 $(runargs)

release:
	# If you are in OS-X, you will need mingw-w64 for cross compiling to windows
	# brew install mingw-w64
	cargo build --release
	# mkdir -p artifacts
	# BINARY_NAME := $(./binary-name.sh)
	# cp ./target/release/wasmer ./artifacts/$(./binary-name.sh)
	# cp ./target/release/wasmer ./artifacts/${BINARY_NAME}

publish-release:
	ghr -t ${GITHUB_TOKEN} -u ${CIRCLE_PROJECT_USERNAME} -r ${CIRCLE_PROJECT_REPONAME} -c ${CIRCLE_SHA1} -delete ${VERSION} ./artifacts/
