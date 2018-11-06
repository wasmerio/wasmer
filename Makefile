.PHONY: spectests clean build install

# This will re-generate the Rust test files based on spectests/*.wast
spectests:
	WASM_GENERATE_SPECTESTS=1 cargo +nightly build

# clean:
#     rm -rf target

build:
	cargo +nightly build

install:
	cargo +nightly install --path .

test:
	cargo +nightly test -- --test-threads=1
