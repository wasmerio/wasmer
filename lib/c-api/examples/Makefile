CFLAGS = -g -I$(shell $(WASMER_DIR)/bin/wasmer config --includedir)
LDFLAGS = -Wl,-rpath,$(shell $(WASMER_DIR)/bin/wasmer config --libdir)
LDLIBS = $(shell $(WASMER_DIR)/bin/wasmer config --libs)

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

.SILENT: clean
.PHONY: clean
clean:
	rm -f \
		instance instance.o \
		imports-exports imports-exports.o \
		exports-function exports-function.o \
		memory memory.o
