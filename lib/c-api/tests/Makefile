WASMER_DIR:=$(realpath $(WASMER_DIR))

$(info Using provided WASMER_DIR=$(WASMER_DIR))

ROOT_DIR:=$(shell dirname $(realpath $(firstword $(MAKEFILE_LIST))))

ifeq (,$(wildcard $(WASMER_DIR)/bin/wasmer))
	CFLAGS = -g -I$(ROOT_DIR)/ -I$(WASMER_DIR)/include
	LDFLAGS = -Wl,-rpath,$(WASMER_DIR)/lib
	LDLIBS = -L$(WASMER_DIR)/lib -lwasmer
else
	CFLAGS = -g -I$(ROOT_DIR)/ -I$(shell $(WASMER_DIR)/bin/wasmer config --includedir)
	LDFLAGS = -Wl,-rpath,$(shell $(WASMER_DIR)/bin/wasmer config --libdir)
	LDLIBS = $(shell $(WASMER_DIR)/bin/wasmer config --libs)
endif

$(info * CFLAGS: $(CFLAGS))
$(info * LDFLAGS: $(LDFLAGS))
$(info * LDLIBS: $(LDLIBS))
$(info * ROOT_DIR: $(ROOT_DIR))
$(info * WASMER_DIR: $(WASMER_DIR))

CAPI_BASE_TESTS = \
	wasm-c-api/example/callback	wasm-c-api/example/global	wasm-c-api/example/hello \
	wasm-c-api/example/memory	wasm-c-api/example/reflect	wasm-c-api/example/serialize \
	wasm-c-api/example/start	wasm-c-api/example/trap		wasm-c-api/example/multi

CAPI_BASE_TESTS_NOT_WORKING = \
	wasm-c-api/example/finalize	wasm-c-api/example/hostref	wasm-c-api/example/threads \
	wasm-c-api/example/table

ALL = $(CAPI_BASE_TESTS)

test: 
	WASMER_DIR="$(WASMER_DIR)" ROOT_DIR="$(ROOT_DIR)" CFLAGS="$(CFLAGS)" LDFLAGS="$(LDFLAGS)" LDLIBS="$(LDLIBS)" cargo test --manifest-path="./wasmer-c-api-test-runner/Cargo.toml" -- --nocapture

.SILENT: clean
.PHONY: clean
clean:
	$(foreach file,$(ALL),rm -f $(file).o $(file))
