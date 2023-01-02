WASMER_DIR:=$(realpath $(WASMER_DIR))

$(info Using provided WASMER_DIR=$(WASMER_DIR))

ROOT_DIR:=$(shell dirname $(realpath $(firstword $(MAKEFILE_LIST))))
ROOT_DIR_PARENT:=$(shell dirname $(ROOT_DIR))

MSVC_CFLAGS:=""
MSVC_LDFLAGS:=""
MSVC_LDLIBS:=""

ifeq (,$(wildcard $(WASMER_DIR)/bin/wasmer))
	CFLAGS = -g -I $(ROOT_DIR)/ -I $(WASMER_DIR)/include
	LDFLAGS = -Wl,-rpath,$(WASMER_DIR)/lib
	LDLIBS = -L $(WASMER_DIR)/lib -lwasmer

	MSVC_CFLAGS:= /DEBUG /I $(ROOT_DIR)/ /I $(WASMER_DIR)/include
	MSVC_LDFLAGS:= ""
	MSVC_LDLIBS:= /LIBPATH:$(WASMER_DIR)/lib wasmer.dll.lib
else
	CFLAGS = -g -I $(ROOT_DIR)/ -I $(shell $(WASMER_DIR)/bin/wasmer config --includedir)
	LDFLAGS = -Wl,-rpath,$(shell $(WASMER_DIR)/bin/wasmer config --libdir)
	LDLIBS = $(shell $(WASMER_DIR)/bin/wasmer config --libs)

	MSVC_CFLAGS:= /DEBUG /I $(ROOT_DIR)/ /I $(shell $(WASMER_DIR)/bin/wasmer config --includedir)
	MSVC_LDFLAGS:= ""
	MSVC_LDLIBS:= /LIBPATH:$(shell $(WASMER_DIR)/bin/wasmer config --libs) wasmer.dll.lib
endif

$(info * CFLAGS: $(CFLAGS))
$(info * LDFLAGS: $(LDFLAGS))
$(info * LDLIBS: $(LDLIBS))

ALL = deprecated-header early-exit instance imports-exports exports-function exports-global memory memory2 features wasi

.PHONY: run
.SILENT: run
run: 	
	WASMER_DIR="$(WASMER_DIR)" ROOT_DIR="$(ROOT_DIR)" CFLAGS="$(CFLAGS)" LDFLAGS="$(LDFLAGS)" LDLIBS="$(LDLIBS)" cargo test --manifest-path="./wasmer-capi-examples-runner/Cargo.toml" -- --nocapture 2>&1

.SILENT: clean
.PHONY: clean
clean:
	$(foreach file,$(ALL),rm -f $(file).o $(file))