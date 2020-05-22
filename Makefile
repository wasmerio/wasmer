tests-spec-update-testsuite:
	git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

test:
	cargo test --release

doc:
	cargo doc --all-features --document-private-items

doc-local:
	cargo doc --all-features --document-private-items --no-deps

RUSTFLAGS := "-D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects" # TODO: add `-D missing-docs`
lint:
	cargo fmt --all -- --check
	RUSTFLAGS=${RUSTFLAGS} cargo clippy

capi-singlepass:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,wasi

capi-cranelift:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features cranelift-backend,wasi

capi-llvm:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features llvm-backend,wasi

# We use cranelift as the default backend for the capi for now
capi: capi-cranelift

test-capi-singlepass: capi-singlepass
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,wasi

test-capi-cranelift: capi-cranelift
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features cranelift-backend,wasi -- --nocapture --test-threads=1

test-capi-llvm: capi-llvm
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features llvm-backend,wasi

test-capi: test-capi-singlepass test-capi-cranelift test-capi-llvm test-capi-emscripten
