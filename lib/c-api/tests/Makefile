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

CAPI_WASMER_TESTS = \
	test-early-exit 					test-memory 							  	test-wasi \

CAPI_BASE_TESTS = \
	wasm-c-api/example/callback			wasm-c-api/example/global					wasm-c-api/example/hello \
	wasm-c-api/example/memory			wasm-c-api/example/reflect					wasm-c-api/example/serialize \
	wasm-c-api/example/start			wasm-c-api/example/trap						wasm-c-api/example/multi

CAPI_BASE_TESTS_NOT_WORKING = \
	wasm-c-api/example/finalize			wasm-c-api/example/hostref					wasm-c-api/example/threads \
	wasm-c-api/example/table

DEPRECATED_TESTS = \
	deprecated/test-context				deprecated/test-import-object				deprecated/test-module-imports \
	deprecated/test-import-trap			deprecated/test-module-serialize \
	deprecated/test-exported-memory		deprecated/test-imports						deprecated/test-module \
	deprecated/test-exports				deprecated/test-instantiate					deprecated/test-tables \
	deprecated/test-globals				deprecated/test-memory						deprecated/test-validate \
	deprecated/test-module-exports		deprecated/test-module-import-instantiate

DEPRECATED_NOT_WORKING = test-import-function test-import-function-callinfo test-wasi-import-object test-emscripten-import-object

ALL = $(CAPI_BASE_TESTS) $(CAPI_WASMER_TESTS) $(DEPRECATED_TESTS)

test-%: %.o
deprecated/test-%: deprecated/%.o

.PHONY: all
all: $(ALL)

.PHONY: test-capi-wasmer
.SILENT: test-capi-wasmer
test-capi-wasmer: $(CAPI_WASMER_TESTS)
	set -o errexit; \
	$(foreach example,$?,echo Running \"$(example)\" example; cd $(shell dirname $(realpath $(example))) && ./$(shell basename $(example)); echo;)

.PHONY: test-capi-base
.SILENT: test-capi-base
test-capi-base: $(CAPI_BASE_TESTS)
	set -o errexit; \
	$(foreach example,$?,echo Running \"$(example)\" example; cd $(shell dirname $(realpath $(example))) && ./$(shell basename $(example)); echo;)

.PHONY: test-capi-base
.SILENT: test-capi-base
test-capi: test-capi-base test-capi-wasmer

.PHONY: test-deprecated
.SILENT: test-deprecated
test-deprecated: $(DEPRECATED_TESTS)
	set -o errexit; \
	$(foreach example,$?,echo Running \"$(example)\" example; cd $(shell dirname $(realpath $(example))) && ./$(shell basename $(example)); echo;)

test: test-capi test-deprecated

.SILENT: clean
.PHONY: clean
clean:
	$(foreach file,$(ALL),rm -f $(file).o $(file))
