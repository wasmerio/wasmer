SHELL=/bin/bash


#####
#
# The Matrix
#
#####

# The matrix is the product of the following columns:
#
# |------------|--------|----------|--------------|-------|
# | Compiler   ⨯ Engine ⨯ Platform ⨯ Architecture ⨯ libc  |
# |------------|--------|----------|--------------|-------|
# | Cranelift  | JIT    | Linux    | amd64        | glibc |
# | LLVM       | Native | Darwin   | aarch64      | musl  |
# | Singlepass |        | Windows  |              |       |
# |------------|--------|----------|--------------|-------|
#
# Here is what works and what doesn't:
#
# * Cranelift with the JIT engine works everywhere,
#
# * Cranelift with the Native engine works on Linux+Darwin/`amd64`,
#   but it doesn't work on */`aarch64` or Windows/*.
#
# * LLVM with the JIT engine works on Linux+Darwin/`amd64`,
#   but it doesn't work on */`aarch64` or Windows/*.
#
# * LLVM with the Native engine works on
#   Linux+Darwin/`amd64`+`aarch64`, but it doesn't work on Windows/*.
#
# * Singlepass with the JIT engine works on Linux+Darwin/`amd64`, but
#   it doesn't work on */`aarch64` or Windows/*.
#
# * Singlepass with the Native engine doesn't work because it doesn't
#   know how to output object files for the moment.
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


# Variables that can be overriden by the users to force to enable or
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
ifneq ($(ENABLE_LLVM), 0)
	# … then maybe the user forced to enable the LLVM compiler.
	ifeq ($(ENABLE_LLVM), 1)
		compilers += llvm
	# … otherwise, we try to autodetect LLVM from `llvm-config`
	else ifneq (, $(shell which llvm-config 2>/dev/null))
		LLVM_VERSION := $(shell llvm-config --version)

		# If findstring is not empty, then it have found the value
		ifneq (, $(findstring 11,$(LLVM_VERSION)))
			compilers += llvm
		else ifneq (, $(findstring 10,$(LLVM_VERSION)))
			compilers += llvm
		endif
	# … or try to autodetect LLVM from `llvm-config-<version>`.
	else
		ifneq (, $(shell which llvm-config-11 2>/dev/null))
			compilers += llvm
		else ifneq (, $(shell which llvm-config-10 2>/dev/null))
			compilers += llvm
		endif
	endif
endif

ifneq (, $(findstring llvm,$(compilers)))
	ENABLE_LLVM := 1
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
	else ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX)))
		ifeq ($(IS_AMD64), 1)
			compilers += singlepass
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
	compilers_engines += cranelift-jit

	ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX)))
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

ifeq ($(ENABLE_LLVM), 1)
	ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX)))
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

ifeq ($(ENABLE_SINGLEPASS), 1)
	ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX)))
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

# Define the compiler Cargo features for all crates.
compiler_features := --features $(subst $(space),$(comma),$(compilers))

# Define the compiler Cargo features for the C API. It always excludes
# LLVM for the moment.
capi_compiler_features := --features $(subst $(space),$(comma),$(filter-out llvm, $(compilers)))


#####
#
# Display information.
#
#####

ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX)))
	bold := $(shell tput bold 2>/dev/null || echo -n '')
	green := $(shell tput setaf 2 2>/dev/null || echo -n '')
	yellow := $(shell tput setaf 3 2>/dev/null || echo -n '')
	reset := $(shell tput sgr0 2>/dev/null || echo -n '')
endif

HOST_TARGET=$(shell rustup show | grep 'Default host: ' | cut -d':' -f2 | tr -d ' ')

TARGET_DIR := target/release

ifneq (, $(TARGET))
	TARGET_DIR := target/$(TARGET)/release
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
$(info Compilers + engines pairs (for testing): $(bold)$(green)${compilers_engines}$(reset))
$(info Cargo features:)
$(info     - Compilers for all crates: `$(bold)$(green)${compiler_features}$(reset)`.)
$(info     - Compilers for the C API: `$(bold)$(green)${capi_compiler_features}$(reset)`.)
$(info     - Default for the C API: `$(bold)$(green)${capi_default_features}$(reset)`.)
$(info )
$(info )
$(info --------------)
$(info $(bold)$(green)RULE EXECUTION$(reset))
$(info --------------)
$(info )
$(info )

############
# Building #
############

# Not really "all", just the default target that builds enough so make install will go through
all: build-wasmer build-capi

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
build-wasmer-headless-minimal: RUSTFLAGS += -C panic=abort
build-wasmer-headless-minimal:
	RUSTFLAGS="${RUSTFLAGS}" xargo build --target $(HOST_TARGET) --release --manifest-path=lib/cli/Cargo.toml --no-default-features --features headless-minimal --bin wasmer-headless
ifeq ($(IS_DARWIN), 1)
	strip -u target/$(HOST_TARGET)/release/wasmer-headless
else
ifeq ($(IS_WINDOWS), 1)
	strip --strip-unneeded target/$(HOST_TARGET)/release/wasmer-headless.exe
else
	strip --strip-unneeded target/$(HOST_TARGET)/release/wasmer-headless
endif
endif

WAPM_VERSION = v0.5.1
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

capi-setup:
ifeq ($(IS_WINDOWS), 1)
  RUSTFLAGS += -C target-feature=+crt-static
endif

build-docs-capi: capi-setup
	cd lib/c-api/doc/deprecated/ && doxygen doxyfile
	RUSTFLAGS="${RUSTFLAGS}" cargo doc --manifest-path lib/c-api/Cargo.toml --no-deps --features wat,jit,object-file,native,cranelift,wasi $(capi_default_features)

build-capi: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,wasi,middlewares $(capi_default_features) $(capi_compiler_features)

build-capi-singlepass: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,singlepass,wasi,middlewares $(capi_default_features)

build-capi-singlepass-jit: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,singlepass,wasi,middlewares $(capi_default_features)

build-capi-singlepass-native: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,singlepass,wasi,middlewares $(capi_default_features)

build-capi-singlepass-object-file: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,object-file,singlepass,wasi,middlewares $(capi_default_features)

build-capi-cranelift: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,cranelift,wasi,middlewares $(capi_default_features)

build-capi-cranelift-system-libffi: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,cranelift,wasi,middlewares,system-libffi $(capi_default_features)

build-capi-cranelift-jit: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,cranelift,wasi,middlewares $(capi_default_features)

build-capi-cranelift-native: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,cranelift,wasi,middlewares $(capi_default_features)

build-capi-cranelift-object-file: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,object-file,cranelift,wasi,middlewares $(capi_default_features)

build-capi-llvm: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,llvm,wasi,middlewares $(capi_default_features)

build-capi-llvm-jit: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,llvm,wasi,middlewares $(capi_default_features)

build-capi-llvm-native: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,llvm,wasi,middlewares $(capi_default_features)

build-capi-llvm-object-file: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,object-file,llvm,wasi,middlewares $(capi_default_features)

# Headless (we include the minimal to be able to run)

build-capi-headless-jit: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features jit,wasi

build-capi-headless-native: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features native,wasi

build-capi-headless-object-file: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features object-file,wasi

build-capi-headless-all: capi-setup
	RUSTFLAGS="${RUSTFLAGS}" cargo build --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features jit,native,object-file,wasi

###########
# Testing #
###########

test: $(foreach compiler,$(compilers),test-$(compiler)) test-packages test-examples test-deprecated

test-singlepass-native:
	cargo test --release --tests $(compiler_features) --features "test-singlepass test-native"

test-singlepass-jit:
	cargo test --release --tests $(compiler_features) --features "test-singlepass test-jit"

test-cranelift-native:
	cargo test --release --tests $(compiler_features) --features "test-cranelift test-native"

test-cranelift-jit:
	cargo test --release --tests $(compiler_features) --features "test-cranelift test-jit"

test-llvm-native:
	cargo test --release --tests $(compiler_features) --features "test-llvm test-native"

test-llvm-jit:
	cargo test --release --tests $(compiler_features) --features "test-llvm test-jit"

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
	cargo test --manifest-path lib/cli/Cargo.toml $(compiler_features) --release
	cargo test -p wasmer-cache --release
	cargo test -p wasmer-engine --release
	cargo test -p wasmer-derive --release
	cargo check --manifest-path fuzz/Cargo.toml $(compiler_features) --release


# We want to run all the tests for all available compilers. The C API
# and the tests rely on the fact that one and only one default
# compiler will be selected at compile-time. Therefore, if we want to
# test exhaustively for all available compilers, we need to build and
# to test the C API with a different compiler each time.
#
# That's exactly what `test-capi` does: it runs `build-capi-*` with
# one compiler, and then it runs `test-capi-*` for that compiler
# specifically.
#
# Why do we need to run `build-capi-*` exactly? One might think that
# `cargo test` would generate a static library (`.a`) to link the
# tests against, but no. `cargo test` has no idea that we need this
# static library, that's normal the library isn't generated. Hence the
# need to run `cargo build` prior to testing to get all the build
# artifacts.
#
# Finally, `test-capi` calls `test-capi-all` that runs the tests for
# the library built with `build-capi`, which is the one we will
# deliver to the users, i.e. the one that may include multiple
# compilers.
test-capi: $(foreach compiler_engine,$(compilers_engines),test-capi-$(compiler_engine)) test-capi-all

test-capi-all: build-capi
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,native,object-file,wasi,middlewares $(capi_default_features) $(capi_compiler_features) -- --nocapture

test-capi-singlepass-jit: build-capi-singlepass-jit test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,singlepass,wasi,middlewares $(capi_default_features) -- --nocapture

test-capi-cranelift-jit: build-capi-cranelift-jit test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,cranelift,wasi,middlewares $(capi_default_features) -- --nocapture

test-capi-cranelift-native: build-capi-cranelift-native test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,cranelift,wasi,middlewares $(capi_default_features) -- --nocapture

test-capi-llvm-jit: build-capi-llvm-jit test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,jit,llvm,wasi,middlewares $(capi_default_features) -- --nocapture

test-capi-llvm-native: build-capi-llvm-native test-capi-tests
	cargo test --manifest-path lib/c-api/Cargo.toml --release \
		--no-default-features --features deprecated,wat,native,llvm,wasi,middlewares $(capi_default_features) -- --nocapture

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
ifneq (, $(filter 1, $(IS_DARWIN) $(IS_LINUX)))
	if [ -d "wapm-cli" ]; then \
		cp wapm-cli/$(TARGET_DIR)/wapm package/bin/ ;\
		echo "#!/bin/bash\nwapm execute \"\$$@\"" > package/bin/wax ;\
		chmod +x package/bin/wax ;\
	fi
else
	if [ -d "wapm-cli" ]; then \
		cp wapm-cli/$(TARGET_DIR)/wapm package/bin/ ;\
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
	cp $(TARGET_DIR)/wasmer.exe package/bin/
else
	cp $(TARGET_DIR)/wasmer package/bin/
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

	# Windows
	if [ -f $(TARGET_DIR)/wasmer_c_api.dll ]; then \
		cp $(TARGET_DIR)/wasmer_c_api.dll package/lib/wasmer.dll ;\
	fi
	if [ -f $(TARGET_DIR)/wasmer_c_api.lib ]; then \
		cp $(TARGET_DIR)/wasmer_c_api.lib package/lib/wasmer.lib ;\
	fi

	# For some reason in macOS arm64 there are issues if we copy constantly in the install_name_tool util
	rm -f package/lib/libwasmer.dylib
	if [ -f $(TARGET_DIR)/libwasmer_c_api.dylib ]; then \
		cp $(TARGET_DIR)/libwasmer_c_api.dylib package/lib/libwasmer.dylib ;\
		install_name_tool -id "@rpath/libwasmer.dylib" package/lib/libwasmer.dylib ;\
	fi

	if [ -f $(TARGET_DIR)/libwasmer_c_api.so ]; then \
		cp $(TARGET_DIR)/libwasmer_c_api.so package/lib/libwasmer.so ;\
	fi
	if [ -f $(TARGET_DIR)/libwasmer_c_api.a ]; then \
		cp $(TARGET_DIR)/libwasmer_c_api.a package/lib/libwasmer.a ;\
	fi

package-docs: build-docs build-docs-capi
	mkdir -p "package/docs/c"
	cp -R target/doc package/docs/crates
	cp -R lib/c-api/doc/deprecated/html/* package/docs/c
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

########################
# (Distro-) Installing #
########################

DESTDIR ?= /usr/local

install: install-wasmer install-capi-headers install-capi-lib install-capi-staticlib install-pkgconfig install-misc

install-wasmer:
	install -Dm755 target/release/wasmer $(DESTDIR)/bin/wasmer

install-capi-headers:
	for header in lib/c-api/*.{h,hh}; do install -Dm644 "$$header" $(DESTDIR)/include/$$(basename $$header); done
	install -Dm644 lib/c-api/README.md $(DESTDIR)/include/wasmer-README.md

# Currently implemented for linux only. TODO
install-capi-lib:
	pkgver=$$(target/release/wasmer --version | cut -d\  -f2) && \
	shortver="$${pkgver%.*}" && \
	majorver="$${shortver%.*}" && \
	install -Dm755 target/release/libwasmer_c_api.so "$(DESTDIR)/lib/libwasmer.so.$$pkgver" && \
	ln -sf "libwasmer.so.$$pkgver" "$(DESTDIR)/lib/libwasmer.so.$$shortver" && \
	ln -sf "libwasmer.so.$$pkgver" "$(DESTDIR)/lib/libwasmer.so.$$majorver" && \
	ln -sf "libwasmer.so.$$pkgver" "$(DESTDIR)/lib/libwasmer.so"

install-capi-staticlib:
	install -Dm644 target/release/libwasmer_c_api.a "$(DESTDIR)/lib/libwasmer.a"

install-misc:
	install -Dm644 LICENSE "$(DESTDIR)"/share/licenses/wasmer/LICENSE

install-pkgconfig:
	unset WASMER_DIR # Make sure WASMER_INSTALL_PREFIX is set during build
	target/release/wasmer config --pkg-config | install -Dm644 /dev/stdin "$(DESTDIR)"/lib/pkgconfig/wasmer.pc

install-wasmer-headless-minimal:
	install -Dm755 target/release/wasmer-headless $(DESTDIR)/bin/wasmer-headless

#################
# Miscellaneous #
#################

# Updates the spectests from the repo
update-testsuite:
	git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

lint-packages: RUSTFLAGS += -D dead-code -D nonstandard-style -D unused-imports -D unused-mut -D unused-variables -D unused-unsafe -D unreachable-patterns -D bad-style -D improper-ctypes -D unused-allocation -D unused-comparisons -D while-true -D unconditional-recursion -D bare-trait-objects # TODO: add `-D missing-docs` # TODO: add `-D function_item_references` (not available on Rust 1.47, try when upgrading)
lint-packages:
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-c-api
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-vm
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-types
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-wasi
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-object
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-engine-native
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-engine-jit
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-compiler
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-compiler-cranelift
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-compiler-singlepass
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy --manifest-path lib/cli/Cargo.toml $(compiler_features)
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-cache
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy -p wasmer-engine
	RUSTFLAGS="${RUSTFLAGS}" cargo clippy --manifest-path fuzz/Cargo.toml $(compiler_features)

lint-formatting:
	cargo fmt --all -- --check
	cargo fmt --manifest-path fuzz/Cargo.toml -- --check

lint: lint-formatting lint-packages

install-local: package
	tar -C ~/.wasmer -zxvf wasmer.tar.gz
