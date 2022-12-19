SHELL=/usr/bin/env bash


#####
#
# The Matrix
#
#####

# The matrix is the product of the following columns:
#
# |------------|----------|--------------|-------|
# | Compiler   ⨯ Platform ⨯ Architecture ⨯ libc  |
# |------------|----------|--------------|-------|
# | Cranelift  | Linux    | amd64        | glibc |
# | LLVM       | Darwin   | aarch64      | musl  |
# | Singlepass | Windows  |              |       |
# |------------|----------|--------------|-------|
#
# Here is what works and what doesn't:
#
# * Cranelift works everywhere,
#
# * LLVM works on Linux+Darwin/`amd64`,
#   but it doesn't work on */`aarch64` or Windows/*.
#
# * Singlepass works on Linux+Darwin/`amd64`, but
#   it doesn't work on */`aarch64` or Windows/*.
#
# * Windows isn't tested on `aarch64`, that's why we consider it's not
#   working, but it might possibly be.


#####
#
# Define the “Platform” and “Architecture” columns of the matrix.
#
#####


IS_DARWIN := 0
IS_LINUX := 0
IS_FREEBSD := 0
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
	else ifeq ($(uname), FreeBSD)
		IS_FREEBSD := 1
	else
		# We use spaces instead of tabs to indent `$(error)`
		# otherwise it's considered as a command outside a
		# target and it will fail.
                $(error Unrecognized platform, expect `Darwin`, `Linux` or `Windows_NT`)
	endif

	# Architecture
	uname := $(shell uname -m)

	ifneq (, $(filter $(uname), x86_64 amd64))
		IS_AMD64 := 1
	else ifneq (, $(filter $(uname), aarch64 arm64))
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

CARGO_BINARY ?= cargo
CARGO_TARGET ?=

# Variables that can be overridden by the users to force to enable or
# to disable a specific compiler.
ENABLE_CRANELIFT ?=
ENABLE_LLVM ?=
ENABLE_SINGLEPASS ?=

# Which compilers we build. These have dependencies that may not be on the system.
compilers := 

##
# Cranelift
##

# If the user didn't disable the Cranelift compiler…
ifneq ($(ENABLE_CRANELIFT), 0)
	# … then it can always be enabled.
	compilers += cranelift
	ENABLE_CRANELIFT := 1
endif

##
# LLVM
##

# If the user didn't disable the LLVM compiler…
ifeq ($(ENABLE_LLVM), 0)
	LLVM_VERSION :=
	# … then maybe the user forced to enable the LLVM compiler.
else ifeq ($(ENABLE_LLVM), 1)
	LLVM_VERSION := $(shell llvm-config --version)
	compilers += llvm
	# … or try to autodetect LLVM from `llvm-config-<version>`.
else ifneq (, $(shell which llvm-config-13 2>/dev/null))
	LLVM_VERSION := $(shell llvm-config-13 --version)
	compilers += llvm
	# need force LLVM_SYS_120_PREFIX, or llvm_sys will not build in the case
	export LLVM_SYS_120_PREFIX = $(shell llvm-config-13 --prefix)
else ifneq (, $(shell which llvm-config-12 2>/dev/null))
	LLVM_VERSION := $(shell llvm-config-12 --version)
	compilers += llvm
	# … otherwise, we try to autodetect LLVM from `llvm-config`
else ifneq (, $(shell which llvm-config 2>/dev/null))
	LLVM_VERSION := $(shell llvm-config --version)
	ifneq (, $(findstring 13,$(LLVM_VERSION)))
		compilers += llvm
	else ifneq (, $(findstring 12,$(LLVM_VERSION)))
		compilers += llvm
	endif
endif

# If findstring is not empty, then it have found the value

exclude_tests := --exclude wasmer-c-api --exclude wasmer-cli --exclude wasmer-compiler-cli
# Is failing to compile in Linux for some reason
exclude_tests += --exclude wasmer-wasi-experimental-io-devices
# We run integration tests separately (it requires building the c-api)
exclude_tests += --exclude wasmer-integration-tests-cli
exclude_tests += --exclude wasmer-integration-tests-ios

ifneq (, $(findstring llvm,$(compilers)))
	ENABLE_LLVM := 1
else
	# We exclude LLVM from our package testing
	exclude_tests += --exclude wasmer-compiler-llvm
endif

##
# Singlepass
##

# If the user didn't disable the Singlepass compiler…
ifneq ($(ENABLE_SINGLEPASS), 0)
	# … then maybe the user forced to enable the Singlepass compiler.
	ifeq ($(ENABLE_SINGLEPASS), 1)
		compilers += singlepass
	# … otherwise, we try to check whether Singlepass works on this host.
	else ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX) $(IS_FREEBSD) $(IS_WINDOWS)))
		ifeq ($(IS_AMD64), 1)
			compilers += singlepass
		endif
		ifeq ($(IS_AARCH64), 1)
			ifneq ($(IS_WINDOWS), 1)
				compilers += singlepass
			endif
		endif
	endif
endif

ifneq (, $(findstring singlepass,$(compilers)))
	ENABLE_SINGLEPASS := 1
endif

##
# Clean the `compilers` variable.
##

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

ifeq ($(ENABLE_CRANELIFT), 1)
	compilers_engines += cranelift-universal
endif

##
# The LLVM case.
##

ifeq ($(ENABLE_LLVM), 1)
	ifneq (, $(filter 1, $(IS_WINDOWS) $(IS_DARWIN) $(IS_LINUX) $(IS_FREEBSD)))
		ifeq ($(IS_AMD64), 1)
			compilers_engines += llvm-universal
		else ifeq ($(IS_AARCH64), 1)
			compilers_engines += llvm-universal
		endif
	endif
endif

##
# The Singlepass case.
##

ifeq ($(ENABLE_SINGLEPASS), 1)
	ifneq (, $(filter 1, $(IS_WINDOWS) $(IS_DARWIN) $(IS_LINUX) $(IS_FREEBSD)))
		ifeq ($(IS_AMD64), 1)
			compilers_engines += singlepass-universal
		endif
		ifeq ($(IS_AARCH64), 1)
			compilers_engines += singlepass-universal
		endif
	endif
endif

# Clean the `compilers_engines` variable.
compilers_engines := $(strip $(compilers_engines))


#####
#
# Cargo features.
#
#####

# Small trick to define a space and a comma.
space := $() $()
comma := ,

# Define the compiler Cargo features for all crates.
compiler_features := --features $(subst $(space),$(comma),$(compilers)),wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load
capi_compilers_engines_exclude := 

# Define the compiler Cargo features for the C API. It always excludes
# LLVM for the moment because it causes the linker to fail since LLVM is not statically linked.
# TODO: Reenable LLVM in C-API
capi_compiler_features := --features $(subst $(space),$(comma),$(filter-out llvm, $(compilers))),wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load
capi_compilers_engines_exclude += llvm-universal

# We exclude singlepass-universal because it doesn't support multivalue (required in wasm-c-api tests)
capi_compilers_engines_exclude += singlepass-universal

capi_compilers_engines := $(filter-out $(capi_compilers_engines_exclude),$(compilers_engines))

#####
#
# Display information.
#
#####

ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX) $(IS_FREEBSD)))
	bold := $(shell tput bold 2>/dev/null || echo -n '')
	green := $(shell tput setaf 2 2>/dev/null || echo -n '')
	yellow := $(shell tput setaf 3 2>/dev/null || echo -n '')
	reset := $(shell tput sgr0 2>/dev/null || echo -n '')
endif

HOST_TARGET=$(shell rustup show | grep 'Default host: ' | cut -d':' -f2 | tr -d ' ')

TARGET_DIR ?= target/release

ifneq (, $(TARGET))
	TARGET_DIR ?= target/$(TARGET)/release
endif

$(info -----------)
$(info $(bold)$(green)INFORMATION$(reset))
$(info -----------)
$(info )
$(info Host Target: `$(bold)$(green)$(HOST_TARGET)$(reset)`.)
ifneq (, $(TARGET))
	# We use spaces instead of tabs to indent `$(info)`
	# otherwise it's considered as a command outside a
	# target and it will fail.
        $(info Build Target: $(bold)$(green)$(TARGET)$(reset) $(yellow)($(TARGET_DIR))$(reset))
endif
ifneq (, $(LIBC))
	# We use spaces instead of tabs to indent `$(info)`
	# otherwise it's considered as a command outside a
	# target and it will fail.
        $(info C standard library: $(bold)$(green)$(LIBC)$(reset))
endif
$(info Enabled Compilers: $(bold)$(green)$(subst $(space),$(reset)$(comma)$(space)$(bold)$(green),$(compilers))$(reset).)
$(info Testing the following compilers & engines:)
$(info   * API: $(bold)$(green)${compilers_engines}$(reset),)
$(info   * C-API: $(bold)$(green)${capi_compilers_engines}$(reset).)
$(info Cargo features:)
$(info   * Compilers: `$(bold)$(green)${compiler_features}$(reset)`.)
$(info Rust version: $(bold)$(green)$(shell rustc --version)$(reset).)
$(info NodeJS version: $(bold)$(green)$(shell node --version)$(reset).)
ifeq ($(ENABLE_LLVM), 1)
        $(info LLVM version: $(bold)$(green)${LLVM_VERSION}$(reset).)
endif
$(info )
$(info )
$(info --------------)
$(info $(bold)$(green)RULE EXECUTION$(reset))
$(info --------------)
$(info )
$(info )

#####
#
# Configure `sed -i` for a cross-platform usage.
#
#####

SEDI ?=

ifeq ($(IS_DARWIN), 1)
	SEDI := "-i ''"
else ifeq ($(IS_FREEBSD), 1)
	SEDI := "-i ''"
else ifeq ($(IS_LINUX), 1)
	SEDI := "-i"
endif

#####
#
# Building.
#
#####

# Not really "all", just the default target that builds enough so make
# install will go through.
all: build-wasmer build-capi

check: check-wasmer check-wasmer-wasm check-capi

check-wasmer:
	$(CARGO_BINARY) check $(CARGO_TARGET) --manifest-path lib/cli/Cargo.toml $(compiler_features) --bin wasmer

check-wasmer-wasm:
	$(CARGO_BINARY) check --manifest-path lib/cli-compiler/Cargo.toml --target wasm32-wasi --features singlepass,cranelift --bin wasmer-compiler

check-capi: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) check $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml  \
		--no-default-features --features wat,compiler,wasi,middlewares $(capi_compiler_features)

build-wasmer:
	$(CARGO_BINARY) build $(CARGO_TARGET) --release --manifest-path lib/cli/Cargo.toml $(compiler_features) --features="webc_runner" --bin wasmer

build-wasmer-debug:
	$(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/cli/Cargo.toml $(compiler_features) --features "webc_runner,debug"  --bin wasmer

bench:
	$(CARGO_BINARY) bench $(CARGO_TARGET) $(compiler_features)

build-wasmer-wasm:
	$(CARGO_BINARY) build --release --manifest-path lib/cli-compiler/Cargo.toml --target wasm32-wasi --features singlepass,cranelift --bin wasmer-compiler

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
build-wasmer-headless-minimal: RUSTFLAGS += -C panic=abort
build-wasmer-headless-minimal:
	RUSTFLAGS="${RUSTFLAGS}" xargo build --target $(HOST_TARGET) --release --manifest-path=lib/cli/Cargo.toml --no-default-features --features headless-minimal --bin wasmer-headless
ifeq ($(IS_DARWIN), 1)
	strip target/$(HOST_TARGET)/release/wasmer-headless
else ifeq ($(IS_WINDOWS), 1)
	strip --strip-unneeded target/$(HOST_TARGET)/release/wasmer-headless.exe
else
	strip --strip-unneeded target/$(HOST_TARGET)/release/wasmer-headless
endif

WAPM_VERSION = v0.5.3
get-wapm:
	[ -d "wapm-cli" ] || git clone --branch $(WAPM_VERSION) https://github.com/wasmerio/wapm-cli.git

build-wapm: get-wapm
ifeq ($(IS_DARWIN), 1)
	# We build it without bundling sqlite, as is included by default in macos
	$(CARGO_BINARY) build $(CARGO_TARGET) --release --manifest-path wapm-cli/Cargo.toml --no-default-features --features "full packagesigning telemetry update-notifications"
else
	$(CARGO_BINARY) build $(CARGO_TARGET) --release --manifest-path wapm-cli/Cargo.toml --features "telemetry update-notifications"
endif

build-docs:
	$(CARGO_BINARY) doc $(CARGO_TARGET) --release $(compiler_features) --document-private-items --no-deps --workspace --exclude wasmer-c-api

capi-setup:
ifeq ($(IS_WINDOWS), 1)
  RUSTFLAGS += -C target-feature=+crt-static
endif

build-docs-capi: capi-setup
	# `wasmer-c-api` lib's name is `wasmer`. To avoid a conflict
	# when generating the documentation, we rename it to its
	# crate's name. Then we restore the lib's name.
	sed "$(SEDI)"  -e 's/name = "wasmer" # ##lib.name##/name = "wasmer_c_api" # ##lib.name##/' lib/c-api/Cargo.toml
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) doc $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --no-deps --features wat,compiler,cranelift,wasi
	sed "$(SEDI)"  -e 's/name = "wasmer_c_api" # ##lib.name##/name = "wasmer" # ##lib.name##/' lib/c-api/Cargo.toml

build-capi: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,wasi,middlewares,webc_runner $(capi_compiler_features)

build-capi-singlepass: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,singlepass,wasi,middlewares,webc_runner

build-capi-singlepass-universal: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,singlepass,wasi,middlewares,webc_runner

build-capi-cranelift: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,cranelift,wasi,middlewares,webc_runner

build-capi-cranelift-universal: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,cranelift,wasi,middlewares,webc_runner

build-capi-llvm: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,llvm,wasi,middlewares,webc_runner

build-capi-llvm-universal: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,llvm,wasi,middlewares,webc_runner

# Headless (we include the minimal to be able to run)

build-capi-headless: capi-setup
ifeq ($(CARGO_TARGET),)
	RUSTFLAGS="${RUSTFLAGS} -C panic=abort -C link-dead-code -C lto -O -C embed-bitcode=yes" $(CARGO_BINARY) build --target $(HOST_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features compiler-headless,wasi
else
	RUSTFLAGS="${RUSTFLAGS} -C panic=abort -C link-dead-code -C lto -O -C embed-bitcode=yes" $(CARGO_BINARY) build $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features compiler-headless,wasi
endif

build-capi-headless-ios: capi-setup
	RUSTFLAGS="${RUSTFLAGS} -C panic=abort" cargo lipo --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features compiler-headless,wasi

#####
#
# Testing.
#
#####

test: test-compilers test-packages test-examples

test-compilers:
	$(CARGO_BINARY) test $(CARGO_TARGET) --release --tests $(compiler_features)

test-packages:
	$(CARGO_BINARY) test $(CARGO_TARGET) --all --release $(exclude_tests)
	$(CARGO_BINARY) test $(CARGO_TARGET) --manifest-path lib/compiler-cranelift/Cargo.toml --release --no-default-features --features=std
	$(CARGO_BINARY) test $(CARGO_TARGET) --manifest-path lib/compiler-singlepass/Cargo.toml --release --no-default-features --features=std
	$(CARGO_BINARY) test $(CARGO_TARGET) --manifest-path lib/cli/Cargo.toml $(compiler_features) --release

test-js: test-js-api test-js-wasi

test-js-core:
	cd lib/api && wasm-pack test --node -- --no-default-features --features js,core,wasm-types-polyfill,wat

test-js-api:
	cd lib/api && wasm-pack test --node -- --no-default-features --features js-default,wat

test-js-wasi:
	cd lib/wasi && wasm-pack test --node -- --no-default-features --features test-js

#####
#
# Testing compilers.
#
#####

test-compilers-compat: $(foreach compiler,$(compilers),test-$(compiler))

test-singlepass-universal:
	$(CARGO_BINARY) test $(CARGO_TARGET) --release --tests $(compiler_features) -- singlepass::universal

test-cranelift-universal:
	$(CARGO_BINARY) test $(CARGO_TARGET) --release --tests $(compiler_features) -- cranelift::universal

test-llvm-universal:
	$(CARGO_BINARY) test $(CARGO_TARGET) --release --tests $(compiler_features) -- llvm::universal

test-singlepass: $(foreach singlepass_engine,$(filter singlepass-%,$(compilers_engines)),test-$(singlepass_engine))

test-cranelift: $(foreach cranelift_engine,$(filter cranelift-%,$(compilers_engines)),test-$(cranelift_engine))

test-llvm: $(foreach llvm_engine,$(filter llvm-%,$(compilers_engines)),test-$(llvm_engine))

# This test requires building the capi with all the available
# compilers first
test-capi: build-capi package-capi $(foreach compiler_engine,$(capi_compilers_engines),test-capi-crate-$(compiler_engine) test-capi-integration-$(compiler_engine))

test-capi-crate-%:
	WASMER_CAPI_CONFIG=$(shell echo $@ | sed -e s/test-capi-crate-//) $(CARGO_BINARY) test $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features wat,compiler,wasi,middlewares,webc_runner $(capi_compiler_features) -- --nocapture

test-capi-integration-%:
	# note: you need to do make build-capi and make package-capi first!
	# Test the Wasmer C API tests for C
	cd lib/c-api/tests; WASMER_CAPI_CONFIG=$(shell echo $@ | sed -e s/test-capi-integration-//) WASMER_DIR=`pwd`/../../../package make test
	# Test the Wasmer C API examples
	cd lib/c-api/examples; WASMER_CAPI_CONFIG=$(shell echo $@ | sed -e s/test-capi-integration-//) WASMER_DIR=`pwd`/../../../package make run

test-wasi-unit:
	$(CARGO_BINARY) test $(CARGO_TARGET) --manifest-path lib/wasi/Cargo.toml --release

test-wasi:
	$(CARGO_BINARY) test $(CARGO_TARGET) --release --tests $(compiler_features) -- wasi::wasitests

test-examples:
	$(CARGO_BINARY) test $(CARGO_TARGET) $(compiler_features) --features wasi --examples
	$(CARGO_BINARY) test $(CARGO_TARGET) --release $(compiler_features) --features wasi --examples

test-integration-cli:
	$(CARGO_BINARY) test $(CARGO_TARGET) --features webc_runner --no-fail-fast -p wasmer-integration-tests-cli -- --nocapture --test-threads=1

test-integration-ios:
	$(CARGO_BINARY) test $(CARGO_TARGET) --features webc_runner -p wasmer-integration-tests-ios

generate-wasi-tests:
# Uncomment the following for installing the toolchain
#   cargo run -p wasi-test-generator -- -s
	$(CARGO_BINARY) run $(CARGO_TARGET) -p wasi-test-generator -- -g
#####
#
# Packaging.
#
#####

package-wapm:
	mkdir -p "package/bin"
ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX) $(IS_FREEBSD)))
	if [ -d "wapm-cli" ]; then \
		cp wapm-cli/$(TARGET_DIR)/wapm package/bin/ ;\
		echo -e "#!/bin/bash\nwapm execute \"\$$@\"" > package/bin/wax ;\
		chmod +x package/bin/wax ;\
	fi
else
	if [ -d "wapm-cli" ]; then \
		cp wapm-cli/$(TARGET_DIR)/wapm package/bin/ ;\
	fi
ifeq ($(IS_DARWIN), 1)
	codesign -s - package/bin/wapm || true
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
	cp $(TARGET_DIR)/wasmer.exe package/bin/
else
	cp $(TARGET_DIR)/wasmer package/bin/
ifeq ($(IS_DARWIN), 1)
	codesign -s - package/bin/wasmer || true
endif
endif

package-capi:
	mkdir -p "package/include"
	mkdir -p "package/lib"
	cp lib/c-api/wasmer.h* package/include
	cp lib/c-api/wasmer_wasm.h* package/include
	cp lib/c-api/tests/wasm-c-api/include/wasm.h* package/include
	cp lib/c-api/README.md package/include/README.md

	if [ -f $(TARGET_DIR)/wasmer.dll ]; then \
		cp $(TARGET_DIR)/wasmer.dll package/lib/wasmer.dll ;\
	fi
	
	if [ -f $(TARGET_DIR)/wasmer.dll.lib ]; then \
		cp $(TARGET_DIR)/wasmer.dll.lib package/lib/wasmer.dll.lib ;\
	fi
	if [ -f $(TARGET_DIR)/wasmer.lib ]; then \
		cp $(TARGET_DIR)/wasmer.lib package/lib/wasmer.lib ;\
	fi

	if [ -f $(TARGET_DIR)/libwasmer.dylib ]; then \
		cp $(TARGET_DIR)/libwasmer.dylib package/lib/libwasmer.dylib ;\
	fi

	if [ -f $(TARGET_DIR)/libwasmer.so ]; then \
		cp $(TARGET_DIR)/libwasmer.so package/lib/libwasmer.so ;\
	fi
	if [ -f $(TARGET_DIR)/libwasmer.a ]; then \
		cp $(TARGET_DIR)/libwasmer.a package/lib/libwasmer.a ;\
	fi

package-capi-headless: build-capi-headless
	mkdir -p "package/include"
	mkdir -p "package/lib"
	cp lib/c-api/wasmer.h* package/include
	cp lib/c-api/wasmer_wasm.h* package/include
	cp lib/c-api/wasm.h* package/include
	cp lib/c-api/README.md package/include/README.md

	if [ -f $(TARGET_DIR)/wasmer.dll ]; then \
		cp $(TARGET_DIR)/wasmer.dll package/lib/wasmer-headless.dll ;\
	fi
	if [ -f $(TARGET_DIR)/wasmer.lib ]; then \
		cp $(TARGET_DIR)/wasmer.lib package/lib/wasmer-headless.lib ;\
	fi

	if [ -f $(TARGET_DIR)/libwasmer.dylib ]; then \
		cp $(TARGET_DIR)/libwasmer.dylib package/lib/libwasmer-headless.dylib ;\
	fi

	if [ -f $(TARGET_DIR)/libwasmer.so ]; then \
		cp $(TARGET_DIR)/libwasmer.so package/lib/libwasmer-headless.so ;\
	fi
	if [ -f $(TARGET_DIR)/libwasmer.a ]; then \
		cp $(TARGET_DIR)/libwasmer.a package/lib/libwasmer-headless.a ;\
	fi

package-docs: build-docs build-docs-capi
	mkdir -p "package/docs/crates"
	cp -R target/doc/ package/docs/crates
	echo '<meta http-equiv="refresh" content="0; url=crates/wasmer/index.html">' > package/docs/index.html
	echo '<meta http-equiv="refresh" content="0; url=wasmer/index.html">' > package/docs/crates/index.html

package: package-wasmer package-minimal-headless-wasmer package-capi

package-gnu: package-capi-gnu

package-capi-gnu:
	mkdir -p "package/include"
	mkdir -p "package/lib"
	cp lib/c-api/wasmer.h* package/include
	cp lib/c-api/wasmer_wasm.h* package/include
	cp lib/c-api/tests/wasm-c-api/include/wasm.h* package/include
	cp lib/c-api/README.md package/include/README.md
	if [ -f target/x86_64-pc-windows-gnu/release/wasmer.dll ]; then \
		cp target/x86_64-pc-windows-gnu/release/wasmer.dll package/lib/wasmer.dll ;\
	fi

	if [ -f target/x86_64-pc-windows-gnu/release/wasmer.dll.lib ]; then \
		cp target/x86_64-pc-windows-gnu/release/wasmer.dll.lib package/lib/wasmer.dll.lib ;\
	fi

	if [ -f target/x86_64-pc-windows-gnu/release/wasmer.lib ]; then \
		cp target/x86_64-pc-windows-gnu/release/wasmer.lib package/lib/wasmer.lib ;\
	fi

	if [ -f target/x86_64-pc-windows-gnu/release/libwasmer.a ]; then \
		cp target/x86_64-pc-windows-gnu/release/libwasmer.a package/lib/libwasmer.a ;\
	fi

distribution-gnu: package-gnu
	cp LICENSE package/LICENSE
	cp ATTRIBUTIONS.md package/ATTRIBUTIONS
	mkdir -p dist
	tar -C package -zcvf wasmer.tar.gz lib include winsdk LICENSE ATTRIBUTIONS
	mv wasmer.tar.gz dist/

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

#####
#
# Installation (for Distros).
#
#####

DESTDIR ?= /usr/local

install: install-wasmer install-capi-headers install-capi-lib install-pkgconfig install-misc

install-capi: install-capi-headers install-capi-lib install-capi-pkgconfig install-misc

install-wasmer:
	install -Dm755 target/release/wasmer $(DESTDIR)/bin/wasmer

install-capi-headers:
	for header in lib/c-api/*.h; do install -Dm644 "$$header" $(DESTDIR)/include/$$(basename $$header); done
	install -Dm644 lib/c-api/README.md $(DESTDIR)/include/wasmer-README.md

# Currently implemented for linux only. TODO
install-capi-lib:
	pkgver=$$($(CARGO_BINARY) pkgid --manifest-path lib/c-api/Cargo.toml | sed --posix 's/^.*wasmer-c-api:\([0-9.]*\)$\/\1/') && \
	shortver="$${pkgver%.*}" && \
	majorver="$${shortver%.*}" && \
	install -Dm755 target/release/libwasmer.so "$(DESTDIR)/lib/libwasmer.so.$$pkgver" && \
	ln -sf "libwasmer.so.$$pkgver" "$(DESTDIR)/lib/libwasmer.so.$$shortver" && \
	ln -sf "libwasmer.so.$$pkgver" "$(DESTDIR)/lib/libwasmer.so.$$majorver" && \
	ln -sf "libwasmer.so.$$pkgver" "$(DESTDIR)/lib/libwasmer.so"

install-misc:
	install -Dm644 LICENSE "$(DESTDIR)"/share/licenses/wasmer/LICENSE

install-capi-pkgconfig:
	@pkgver=$$($(CARGO_BINARY) pkgid --manifest-path lib/c-api/Cargo.toml | sed --posix 's/^.*wasmer-c-api:\([0-9.]*\)$\/\1/') && \
	printf "prefix=%s\nincludedir=\044{prefix}/include\nlibdir=\044{prefix}/lib\n\nName: wasmer\nDescription: The Wasmer library for running WebAssembly\nVersion: %s\nCflags: -I\044{prefix}/include\nLibs: -L\044{prefix}/lib -lwasmer\n" "$(DESTDIR)" "$${pkgver}" | install -Dm644 /dev/stdin "$(DESTDIR)"/lib/pkgconfig/wasmer.pc

install-pkgconfig:
	# Make sure WASMER_INSTALL_PREFIX is set during build
	unset WASMER_DIR; \
	if pc="$$(target/release/wasmer config --pkg-config 2>/dev/null)"; then \
		echo "$$pc" | install -Dm644 /dev/stdin "$(DESTDIR)"/lib/pkgconfig/wasmer.pc; \
	else \
		echo 1>&2 "WASMER_INSTALL_PREFIX was not set during build, not installing wasmer.pc"; \
	fi

install-wasmer-headless-minimal:
	install -Dm755 target/release/wasmer-headless $(DESTDIR)/bin/wasmer-headless

#####
#
# Miscellaneous.
#
#####

# Updates the spectests from the repo
update-testsuite:
	git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

lint-packages: RUSTFLAGS += -D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects -D function_item_references # TODO: add `-D missing-docs`
lint-packages:
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy --all --exclude wasmer-cli -- -D clippy::all
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy --manifest-path lib/cli/Cargo.toml $(compiler_features) -- -D clippy::all
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy --manifest-path fuzz/Cargo.toml $(compiler_features) -- -D clippy::all

lint-formatting:
	cargo fmt --all -- --check
	cargo fmt --manifest-path fuzz/Cargo.toml -- --check

lint: lint-formatting lint-packages

install-local: package
	tar -C ~/.wasmer -zxvf wasmer.tar.gz

test-minimal-versions:
	rm -f Cargo.lock
	cargo +nightly build --tests -Z minimal-versions --all-features

update-graphql-schema:
	curl -sSfL https://registry.wapm.io/graphql/schema.graphql > lib/registry/graphql/schema.graphql
