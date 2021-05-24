WASMER_DIR:=$(realpath $(WASMER_DIR))

$(info Using provided WASMER_DIR=$(WASMER_DIR))

ROOT_DIR:=$(shell dirname $(realpath $(firstword $(MAKEFILE_LIST))))

ifeq (,$(wildcard $(WASMER_DIR)/bin/wasmer))
	CFLAGS = -g -I$(ROOT_DIR)/../tests -I$(WASMER_DIR)/include
	LDFLAGS = -Wl,-rpath,$(WASMER_DIR)/lib
	LDLIBS = -L$(WASMER_DIR)/lib -lwasmer
else
	CFLAGS = -g -I$(ROOT_DIR)/../tests -I$(shell $(WASMER_DIR)/bin/wasmer config --includedir)
	LDFLAGS = -Wl,-rpath,$(shell $(WASMER_DIR)/bin/wasmer config --libdir)
	LDLIBS = $(shell $(WASMER_DIR)/bin/wasmer config --libs)
endif

$(info * CFLAGS: $(CFLAGS))
$(info * LDFLAGS: $(LDFLAGS))
$(info * LDLIBS: $(LDLIBS))

ALL = instance imports-exports exports-function exports-global memory features

.SILENT: instance instance.o
instance: instance.o

.SILENT: imports-exports imports-exports.o
imports-exports: imports-exports.o

.SILENT: exports-function exports-function.o
exports-function: exports-function.o

.SILENT: exports-global exports-global.o
exports-global: exports-global.o

.SILENT: memory memory.o
memory: memory.o

.SILENT: features features.o
features: features.o

.PHONY: all
all: $(ALL)

.PHONY: run
.SILENT: run
run: $(ALL)
	set -o errexit; \
	$(foreach example,$?,echo Running \"$(example)\" example; ./$(example); echo;)

.SILENT: clean
.PHONY: clean
clean:
	$(foreach file,$(ALL),rm -f $(file).o $(file))
