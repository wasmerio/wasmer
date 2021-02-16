SHELL=/bin/bash


#####
#
# The Matrix
#
#####


# |----------|--------------|------------|-------------|------------|------------|
# | Platform | Architecture | Compiler   | Engine      | libc       | Supported? |
# |----------|--------------|------------|-------------|------------|------------|
# | Linux    | amd64        | Cranelift  | JIT         | glibc      |        yes |
# |          |              |            |             | musl       |         no |
# |          |              |            | Native      | glibc      |        yes |
# |          |              |            |             | musl       |         no |
# |          |              |            |             |            |            |
# |          |              | LLVM       | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |        yes |
# |          |              |            |             |            |            |
# |          |              | Singlepass | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |         no |
# |          |              |            |             |            |            |
# |          | aarch64      | Cranelift  | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |         no |
# |          |              |            |             |            |            |
# |          |              | LLVM       | JIT         | glibc      |         no |
# |          |              |            | Native      | glibc      |        yes |
# |          |              |            |             |            |            |
# |          |              | Singlepass | JIT         | glibc      |         no |
# |          |              |            | Native      | glibc      |         no |
#-|----------|--------------|------------|-------------|------------|------------|
# | Darwin   | amd64        | Cranelift  | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |        yes |
# |          |              |            |             |            |            |
# |          |              | LLVM       | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |        yes |
# |          |              |            |             |            |            |
# |          |              | Singlepass | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |         no |
# |          |              |            |             |            |            |
# |          | aarch64      | Cranelift  | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |         no |
# |          |              |            |             |            |            |
# |          |              | LLVM       | JIT         | glibc      |         no |
# |          |              |            | Native      | glibc      |        yes |
# |          |              |            |             |            |            |
# |          |              | Singlepass | JIT         | glibc      |         no |
# |          |              |            | Native      | glibc      |         no |
#-|----------|--------------|------------|-------------|------------|------------|
# | Windows  | amd64        | Cranelift  | JIT         | glibc      |        yes |
# |          |              |            | Native      | glibc      |         no |
# |          |              |            |             |            |            |
# |          |              | LLVM       | JIT         | glibc      |         no |
# |          |              |            | Native      | glibc      |         no |
# |          |              |            |             |            |            |
# |          |              | Singlepass | JIT         | glibc      |         no |
# |          |              |            | Native      | glibc      |         no |
# |----------|--------------|------------|-------------|------------|------------|


#####
#
# Define the “Platform” and “Architecture” columns of the matrix.
#
#####


IS_DARWIN := 0
IS_LINUX := 0
IS_WINDOWS := 0
IS_AMD64 := 0
IS_AARCH64 := 0

# Test Windows apart because it doesn't support `uname -s`.
ifeq ($(OS), Windows_NT)
	# We can assume it will likely be in amd64.
	IS_AMD64 := 1
	IS_WINDOWS := 1
else
	# Platform
	uname := $(shell uname -s)

	ifeq ($(uname), Darwin)
		IS_DARWIN := 1
	else ifeq ($(uname), Linux)
		IS_LINUX := 1
	else
		# We use spaces instead of tabs to indent `$(error)`
		# otherwise it's considered as a command outside a
		# target and it will fail.
                $(error Unrecognized platform, expect `Darwin`, `Linux` or `Windows_NT`)
	endif

	# Architecture
	uname := $(shell uname -m)

	ifeq ($(uname), x86_64)
		IS_AMD64 := 1
	else ifneq (,$(filter $(uname),aarch64 arm64))
		IS_AARCH64 := 1
	else
		# We use spaces instead of tabs to indent `$(error)`
		# otherwise it's considered as a command outside a
		# target and it will fail.
                $(error Unrecognized architecture, expect `x86_64`, `aarch64` or `arm64`)
	endif

	# Libc
	LIBC ?= $(shell ldd 2>&1 | grep -o musl | head -n1)
endif


#####
#
# Define the “Compiler” column of the matrix.
#
#####


HAS_CRANELIFT := 0
HAS_LLVM := 0
HAS_SINGLEPASS := 0

# Which compilers we build. These have dependencies that may not be on the system.
compilers := 

##
# Cranelift
##

compilers += cranelift
HAS_CRANELIFT := 1

##
# LLVM
##

# Autodetect LLVM from `llvm-config`
ifneq (, $(shell which llvm-config 2>/dev/null))
	LLVM_VERSION := $(shell llvm-config --version)

	# If findstring is not empty, then it have found the value
	ifneq (, $(findstring 11,$(LLVM_VERSION)))
		compilers += llvm
	else ifneq (, $(findstring 10,$(LLVM_VERSION)))
		compilers += llvm
	endif
# Autodetect LLVM from `llvm-config-<version>`.
else
	ifneq (, $(shell which llvm-config-11 2>/dev/null))
		compilers += llvm
	else ifneq (, $(shell which llvm-config-10 2>/dev/null))
		compilers += llvm
	endif
endif

ifneq (, $(findstring llvm,$(compilers)))
	HAS_LLVM := 1
endif

##
# Singlepass
##

ifeq ($(IS_WINDOWS), 0)
	ifeq ($(IS_AMD64), 1)
		compilers += singlepass
	endif
endif

ifneq (, $(findstring singlepass,$(compilers)))
	HAS_SINGLEPASS := 1
endif

# Clean the `compilers` variable.
compilers := $(strip $(compilers))


#####
#
# Define the “Engine” column of the matrix.
#
#####


# The engine is part of a pair of kind (compiler, engine). All the
# pairs are stored in the `compilers_engines` variable.
compilers_engines :=

##
# The Cranelift case.
##

ifeq ($(HAS_CRANELIFT), 1)
	compilers_engines += cranelift-jit

	ifeq ($(IS_WINDOWS), 0)
		ifeq ($(IS_AMD64), 1)
			ifneq ($(LIBC), musl)
				compilers_engines += cranelift-native
			endif
		endif
	endif
endif

##
# The LLVM case.
##

ifeq ($(HAS_LLVM), 1)
	ifeq ($(IS_WINDOWS), 0)
		ifeq ($(IS_AMD64), 1)
			compilers_engines += llvm-jit
			compilers_engines += llvm-native
		else ifeq ($(IS_AARCH64), 1)
			compilers_engines += llvm-native
		endif
	endif
endif

##
# The Singlepass case.
##

ifeq ($(HAS_SINGLEPASS), 1)
	ifeq ($(IS_WINDOWS), 0)
		ifeq ($(IS_AMD64), 1)
			compilers_engines += singlepass-jit
		endif
	endif
endif

# Clean the `compilers_engines` variable.
compilers_engines := $(strip $(compilers_engines))


#####
#
# Miscellaneous.
#
#####

# The `libffi` library doesn't support Darwin/aarch64. In this
# particular case, we need to use the `libffi` version provided by the
# system itself.
#
# See <https://github.com/libffi/libffi/pull/621>.
use_system_ffi := 0

ifeq ($(IS_DARWIN), 1)
	ifeq ($(IS_AARCH64), 1)
		use_system_ffi = 1
	endif
endif

# If the user has set the `WASMER_CAPI_USE_SYSTEM_LIBFFI` environment
# variable to 1, then also use the system `libffi`.
ifeq ($(WASMER_CAPI_USE_SYSTEM_LIBFFI), 1)
	use_system_ffi = 1
endif

#####
#
# Cargo features.
#
#####

# Define the default Cargo features for the `wasmer-c-api` crate.
ifeq ($(use_system_ffi), 1)
	capi_default_features := --features system-libffi
endif

# Small trick to define a space and a comma.
space := $() $()
comma := ,

# Define the default Cargo features for all crates.
compiler_features := --features $(subst $(space),$(comma),$(compilers))


#####
#
# Display information.
#
#####

ifeq ($(IS_WINDOWS), 0)
	bold := $(shell tput bold 2>/dev/null || echo -n '')
	green := $(shell tput setaf 2 2>/dev/null || echo -n '')
	reset := $(shell tput sgr0 2>/dev/null || echo -n '')
endif

HOST_TARGET=$(shell rustup show | grep 'Default host: ' | cut -d':' -f2 | tr -d ' ')

ifneq (, $(LIBC))
	$(info C standard library: $(bold)$(green)$(LIBC)$(reset))
endif

$(info Host target: $(bold)$(green)$(HOST_TARGET)$(reset))
$(info Available compilers: $(bold)$(green)${compilers}$(reset))
$(info Compilers features: $(bold)$(green)${compiler_features}$(reset))
$(info Available compilers + engines for test: $(bold)$(green)${compilers_engines}$(reset))
$(info C API default features: $(bold)$(green)${capi_default_features}$(reset))


############
# Building #
############

bench:
	cargo bench $(compiler_features)

build-wasmer:
	cargo build --release --manifest-path lib/cli/Cargo.toml $(compiler_features) --bin wasmer

build-wasmer-debug:
	cargo build --manifest-path lib/cli/Cargo.toml $(compiler_features) --bin wasmer

# For best results ensure the release profile looks like the following
# in Cargo.toml:
# [profile.release]
# opt-level = 'z'
# debug = false
# debug-assertions = false
# overflow-checks = false
# lto = true
# panic = 'abort'
# incremental = false
# codegen-units = 1
# rpath = false
build-wasmer-headless-minimal:
	RUSTFLAGS="-C panic=abort" xargo build --target $(HOST_TARGET) --release --manifest-path=lib/cli/Cargo.toml --no-default-features --features headless-minimal --bin wasmer-headless
ifeq ($(IS_DARWIN), 1)
	strip -u target/$(HOST_TARGET)/release/wasmer-headless
else
ifeq ($(IS_WINDOWS), 1)
	strip --strip-unneeded target/$(HOST_TARGET)/release/wasmer-headless.exe
else
	strip --strip-unneeded target/$(HOST_TARGET)/release/wasmer-headless
endif
endif

WAPM_VERSION = master # v0.5.0
get-wapm:
	[ -d "wapm-cli" ] || git clone --branch $(WAPM_VERSION) https://github.com/wasmerio/wapm-cli.git

build-wapm: get-wapm
ifeq ($(IS_DARWIN), 1)
	# We build it without bundling sqlite, as is included by default in macos
	cargo build --release --manifest-path wapm-cli/Cargo.toml --no-default-features --features "packagesigning telemetry update-notifications"
else
	cargo build --release --manifest-path wapm-cli/Cargo.toml --features "telemetry update-notifications"
endif

build-docs:
	cargo doc --release $(compiler_features) --document-private-items --no-deps --workspace

build-docs-capi:
	cd lib/c-api/doc/deprecated/ && doxygen doxyfile
	cargo doc --manifest-path lib/c-api/Cargo.toml --no-deps --features wat,jit,object-file,native,cranelift,wasi $(capi_default_features)

build-capi:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,wasi $(capi_default_features) $(compiler_features)

build-capi-singlepass:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,singlepass,wasi $(capi_default_features)

build-capi-singlepass-jit:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,singlepass,wasi $(capi_default_features)

build-capi-singlepass-native:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,singlepass,wasi $(capi_default_features)

build-capi-singlepass-object-file:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,object-file,singlepass,wasi $(capi_default_features)

build-capi-cranelift:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,cranelift,wasi $(capi_default_features)

build-capi-cranelift-system-libffi:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,cranelift,wasi,system-libffi $(capi_default_features)

build-capi-cranelift-jit:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,cranelift,wasi $(capi_default_features)

build-capi-cranelift-native:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,cranelift,wasi $(capi_default_features)

build-capi-cranelift-object-file:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,object-file,cranelift,wasi $(capi_default_features)

build-capi-llvm:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,llvm,wasi $(capi_default_features)

build-capi-llvm-jit:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,llvm,wasi $(capi_default_features)

build-capi-llvm-native:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,llvm,wasi $(capi_default_features)

build-capi-llvm-object-file:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,object-file,llvm,wasi $(capi_default_features)

# Headless (we include the minimal to be able to run)

build-capi-headless-jit:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features jit,wasi

build-capi-headless-native:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features native,wasi

build-capi-headless-object-file:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features object-file,wasi

build-capi-headless-all:
	cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features jit,native,object-file,wasi

###########
# Testing #
###########

test: $(foreach compiler,$(compilers),test-$(compiler)) test-packages test-examples test-deprecated

test-singlepass-native:
	cargo test --release $(compiler_features) --features "test-singlepass test-native"

test-singlepass-jit:
	cargo test --release $(compiler_features) --features "test-singlepass test-jit"

test-cranelift-native:
	cargo test --release $(compiler_features) --features "test-cranelift test-native"

test-cranelift-jit:
	cargo test --release $(compiler_features) --features "test-cranelift test-jit"

test-llvm-native:
	cargo test --release $(compiler_features) --features "test-llvm test-native"

test-llvm-jit:
	cargo test --release $(compiler_features) --features "test-llvm test-jit"

test-singlepass: $(foreach singlepass_engine,$(filter singlepass-%,$(compilers_engines)),test-$(singlepass_engine))

test-cranelift: $(foreach cranelift_engine,$(filter cranelift-%,$(compilers_engines)),test-$(cranelift_engine))

test-llvm: $(foreach llvm_engine,$(filter llvm-%,$(compilers_engines)),test-$(llvm_engine))

test-packages:
	cargo test -p wasmer --release
	cargo test -p wasmer-vm --release
	cargo test -p wasmer-types --release
	cargo test -p wasmer-wasi --release
	cargo test -p wasmer-object --release
	cargo test -p wasmer-engine-native --release --no-default-features
	cargo test -p wasmer-engine-jit --release --no-default-features
	cargo test -p wasmer-compiler --release
	cargo test -p wasmer-cli --release
	cargo test -p wasmer-cache --release
	cargo test -p wasmer-engine --release
	cargo test -p wasmer-derive --release


# We want to run all the tests for all available compilers. The C API
# and the tests rely on the fact that one and only one default
# compiler will be selected at compile-time. Therefore, if we want to
# test exhaustively for all available compilers, we need to build and
# to test the C API with a different compiler each time.
#
# That's exactly what `test-capi` does: it runs `test-capi-*` rules
# that, prior to testing, builds the C API with `build-capi-*` sibling
# rules. Why? Because the tests need a static library (`.a` files) to
# link the tests against; `cargo test` doesn't generate such library,
# only `cargo build`.
#
# Finally, `test-capi` calls `test-capi-all` that runs the tests for
# the library built with `build-capi`, which is the one we will
# deliver to the users.
test-capi: $(foreach compiler_engine,$(compilers_engines),test-capi-$(compiler_engine)) test-capi-all

test-capi-all: build-capi
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,wasi $(capi_default_features) $(compiler_features) -- --nocapture

test-capi-singlepass-jit: build-capi-singlepass-jit test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,singlepass,wasi $(capi_default_features) -- --nocapture

test-capi-cranelift-jit: build-capi-cranelift-jit test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,cranelift,wasi $(capi_default_features) -- --nocapture

test-capi-cranelift-native: build-capi-cranelift-native test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,cranelift,wasi $(capi_default_features) -- --nocapture

test-capi-llvm-jit: build-capi-llvm-jit test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,llvm,wasi $(capi_default_features) -- --nocapture

test-capi-llvm-native: build-capi-llvm-native test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,llvm,wasi $(capi_default_features) -- --nocapture

test-capi-tests: package-capi
	# Test the Wasmer C API tests for C
	cd lib/c-api/tests; WASMER_DIR=`pwd`/../../../package make test
	# Test the Wasmer C API examples
	cd lib/c-api/examples; WASMER_DIR=`pwd`/../../../package make run

test-wasi-unit:
	cargo test --manifest-path lib/wasi/Cargo.toml --release

test-examples:
	cargo test --release $(compiler_features) --features wasi --examples

test-deprecated:
	cargo test --manifest-path lib/deprecated/runtime-core/Cargo.toml -p wasmer-runtime-core --release
	cargo test --manifest-path lib/deprecated/runtime/Cargo.toml -p wasmer-runtime --release
	cargo test --manifest-path lib/deprecated/runtime/Cargo.toml -p wasmer-runtime --release --examples

test-integration:
	cargo test -p wasmer-integration-tests-cli

#############
# Packaging #
#############

package-wapm:
	mkdir -p "package/bin"
ifeq ($(IS_WINDOWS), 0)
	if [ -d "wapm-cli" ]; then \
		cp wapm-cli/target/release/wapm package/bin/ ;\
		echo "#!/bin/bash\nwapm execute \"\$$@\"" > package/bin/wax ;\
		chmod +x package/bin/wax ;\
	fi
else
	if [ -d "wapm-cli" ]; then \
		cp wapm-cli/target/release/wapm package/bin/ ;\
	fi
ifeq ($(IS_DARWIN), 1)
	codesign -s - package/bin/wapm
endif
endif

package-minimal-headless-wasmer:
ifeq ($(IS_WINDOWS), 1)
	if [ -f "target/$(HOST_TARGET)/release/wasmer-headless.exe" ]; then \
		cp target/$(HOST_TARGET)/release/wasmer-headless.exe package/bin ;\
	fi
else
	if [ -f "target/$(HOST_TARGET)/release/wasmer-headless" ]; then \
		cp target/$(HOST_TARGET)/release/wasmer-headless package/bin ;\
	fi
endif

package-wasmer:
	mkdir -p "package/bin"
ifeq ($(IS_WINDOWS), 1)
	cp target/release/wasmer.exe package/bin/
else
	cp target/release/wasmer package/bin/
ifeq ($(IS_DARWIN), 1)
	codesign -s - package/bin/wasmer
endif
endif

package-capi:
	mkdir -p "package/include"
	mkdir -p "package/lib"
	cp lib/c-api/wasmer.h* package/include
	cp lib/c-api/wasmer_wasm.h* package/include
	cp lib/c-api/wasm.h* package/include
	cp lib/c-api/README.md package/include/README.md
ifeq ($(IS_WINDOWS), 1)
	cp target/release/wasmer_c_api.dll package/lib/wasmer.dll
	cp target/release/wasmer_c_api.lib package/lib/wasmer.lib
else
ifeq ($(IS_DARWIN), 1)
	# For some reason in macOS arm64 there are issues if we copy constantly in the install_name_tool util
	rm -f package/lib/libwasmer.dylib
	cp target/release/libwasmer_c_api.dylib package/lib/libwasmer.dylib
	cp target/release/libwasmer_c_api.a package/lib/libwasmer.a
	# Fix the rpath for the dylib
	install_name_tool -id "@rpath/libwasmer.dylib" package/lib/libwasmer.dylib
else
	# In some cases the .so may not be available, for example when building against musl (static linking)
	if [ -f target/release/libwasmer_c_api.so ]; then \
		cp target/release/libwasmer_c_api.so package/lib/libwasmer.so ;\
	fi;
	cp target/release/libwasmer_c_api.a package/lib/libwasmer.a
endif
endif

package-docs: build-docs build-docs-capi
	mkdir -p "package/docs"
	mkdir -p "package/docs/c/runtime-c-api"
	cp -R target/doc package/docs/crates
	cp -R lib/c-api/doc/deprecated/html/ package/docs/c/runtime-c-api
	echo '<!-- Build $(SOURCE_VERSION) --><meta http-equiv="refresh" content="0; url=crates/wasmer/index.html">' > package/docs/index.html
	echo '<!-- Build $(SOURCE_VERSION) --><meta http-equiv="refresh" content="0; url=wasmer/index.html">' > package/docs/crates/index.html

package: package-wapm package-wasmer package-minimal-headless-wasmer package-capi

distribution: package
	cp LICENSE package/LICENSE
	cp ATTRIBUTIONS.md package/ATTRIBUTIONS
	mkdir -p dist
ifeq ($(IS_WINDOWS), 1)
	iscc scripts/windows-installer/wasmer.iss
	cp scripts/windows-installer/WasmerInstaller.exe dist/
endif
	tar -C package -zcvf wasmer.tar.gz bin lib include LICENSE ATTRIBUTIONS
	mv wasmer.tar.gz dist/

#################
# Miscellaneous #
#################

# Updates the spectests from the repo
update-testsuite:
	git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

RUSTFLAGS := "-D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects" # TODO: add `-D missing-docs` # TODO: add `-D function_item_references` (not available on Rust 1.47, try when upgrading)
lint-packages:
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-c-api
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-vm
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-types
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-wasi
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-object
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-engine-native
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-engine-jit
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-compiler
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-compiler-cranelift
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-compiler-singlepass
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-cli
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-cache
	RUSTFLAGS=${RUSTFLAGS} cargo clippy -p wasmer-engine

lint-formatting:
	cargo fmt --all -- --check

lint: lint-formatting lint-packages

install-local: package
	tar -C ~/.wasmer -zxvf wasmer.tar.gz
