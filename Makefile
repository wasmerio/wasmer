.PHONY: bench

# uname only works in *Unix like systems
ifneq ($(OS), Windows_NT)
  ARCH := $(shell uname -m)
  UNAME_S := $(shell uname -s)
else
  # We can assume, if in windows it will likely be in x86_64
  ARCH := x86_64
  UNAME_S := 
endif

compilers :=

# Singlepass is enabled
RUST_VERSION := $(shell rustc -V)

ifneq (, $(findstring nightly,$(RUST_VERSION)))
  # Singlepass doesn't work yet on Windows
  ifneq ($(OS), Windows_NT)
    compilers += singlepass
  endif
endif

ifeq ($(ARCH), x86_64)
  # In X64, Cranelift is enabled
  compilers += cranelift
  # LLVM could be enabled if not in Windows
  ifneq ($(OS), Windows_NT)
    # Autodetect LLVM from llvm-config
    ifneq (, $(shell which llvm-config))
      LLVM_VERSION := $(shell llvm-config --version)
      # If findstring is not empty, then it have found the value
      ifneq (, $(findstring 10,$(LLVM_VERSION)))
        compilers += llvm
      endif
    else
      ifneq (, $(shell which llvm-config-10))
        compilers += llvm
      endif
    endif
  endif
endif

compilers := $(filter-out ,$(compilers))

ifneq ($(OS), Windows_NT)
  bold := $(shell tput bold)
  green := $(shell tput setaf 2)
  reset := $(shell tput sgr0)
endif


$(info Available compilers: $(bold)$(green)${compilers}$(reset))

compiler_features_spaced := $(foreach compiler,$(compilers),$(compiler))
compiler_features := --features "$(compiler_features_spaced)"


tests-spec-update-testsuite:
	git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

test:
	cargo test --release $(compiler_features)

bench:
	cargo bench --features "jit" $(compiler_features)

check-bench:
	cargo check --benches --features "jit" $(compiler_features)

release:
	cargo build --release $(compiler_features)

doc:
	cargo doc --all-features --document-private-items

doc-local:
	cargo doc --all-features --document-private-items --no-deps

RUSTFLAGS := "-D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects" # TODO: add `-D missing-docs`
lint:
	cargo fmt --all -- --check
	RUSTFLAGS=${RUSTFLAGS} cargo clippy $(compiler_features)

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
