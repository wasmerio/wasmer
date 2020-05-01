tests-spec-update-testsuite:
	git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

test:
	cargo test --release

doc:
	cargo doc --all-features --document-private-items

capi-singlepass:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,wasi

capi-cranelift:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features cranelift-backend,wasi

capi-llvm:
	cargo build --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features llvm-backend,wasi

# We use cranelift as the default backend for the capi for now
capi: capi-cranelift

test-capi-singlepass: capi-singlepass
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features singlepass-backend,wasi

test-capi-cranelift: capi-cranelift
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features cranelift-backend,wasi

test-capi-llvm: capi-llvm
	cargo test --manifest-path lib/runtime-c-api/Cargo.toml --release \
		--no-default-features --features llvm-backend,wasi

test-capi: test-capi-singlepass test-capi-cranelift test-capi-llvm test-capi-emscripten
