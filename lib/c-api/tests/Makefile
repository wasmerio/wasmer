WASMER_DIR:=$(realpath $(WASMER_DIR))

$(info Using provided WASMER_DIR=$(WASMER_DIR))

ROOT_DIR:=$(shell dirname $(realpath $(firstword $(MAKEFILE_LIST))))
ROOT_DIR_PARENT:=$(shell dirname $(ROOT_DIR))

ifeq (,$(wildcard $(WASMER_DIR)/bin/wasmer))
	CFLAGS = -g -I $(ROOT_DIR) -I $(WASMER_DIR)/include
	LDFLAGS = -Wl,-rpath,$(WASMER_DIR)/lib
	LDLIBS = -L $(WASMER_DIR)/lib -lwasmer
else
	CFLAGS = -g -I $(ROOT_DIR)/wasm-c-api/include/ -I $(shell $(WASMER_DIR)/bin/wasmer config --includedir)
	LDFLAGS = -Wl,-rpath,$(shell $(WASMER_DIR)/bin/wasmer config --libdir)
	LDLIBS = $(shell $(WASMER_DIR)/bin/wasmer config --libs)
endif


$(info * ROOT_DIR: $(ROOT_DIR))
$(info * WASMER_DIR: $(WASMER_DIR))
$(info * "")
$(info * CFLAGS: $(CFLAGS))
$(info * LDFLAGS: $(LDFLAGS))
$(info * LDLIBS: $(LDLIBS))

test: 
	cargo test --manifest-path="./wasmer-c-api-test-runner/Cargo.toml" -- --nocapture 2>&1

.SILENT: clean
.PHONY: clean
clean:
	$(foreach file,$(ALL),rm -f $(file).o $(file))