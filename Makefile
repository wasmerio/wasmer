.PHONY: spectests clean build install

# This will re-generate the Rust test files based on spectests/*.wast
spectests:
	WASM_GENERATE_SPECTESTS=1 cargo build

# clean:
#     rm -rf target

build:
	cargo build

install:
	cargo install --path .

test:
	cargo test
