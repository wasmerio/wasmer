$(info Using provided WASMER_DIR=$(WASMER_DIR))

ifeq (,$(wildcard $(WASMER_DIR)/bin/wasmer))
	CFLAGS = -g -I$(WASMER_DIR)/include
	LDFLAGS = -Wl,-rpath,$(WASMER_DIR)/lib
	LDLIBS = -L$(WASMER_DIR)/lib -lwasmer
else
	CFLAGS = -g -I$(shell $(WASMER_DIR)/bin/wasmer config --includedir)
	LDFLAGS = -Wl,-rpath,$(shell $(WASMER_DIR)/bin/wasmer config --libdir)
	LDLIBS = $(shell $(WASMER_DIR)/bin/wasmer config --libs)
endif

$(info * CFLAGS: $(CFLAGS))
$(info * LDFLAGS: $(LDFLAGS))
$(info * LDLIBS: $(LDLIBS))

ALL = \
	test-context                   test-import-object             test-module-imports \
    test-import-trap               test-module-serialize \
	test-exported-memory           test-imports                   test-module \
	test-exports                   test-instantiate               test-tables \
	test-globals                   test-memory                    test-validate \
	test-module-exports            test-module-import-instantiate

EMSCRIPTEN = test-emscripten-import-object

NOT_WORKING = test-import-function test-import-function-callinfo test-wasi-import-object

test-%: %.o

.PHONY: all
all: $(ALL)

.PHONY: test
.SILENT: test
test: $(ALL)
	set -o errexit; \
	$(foreach example,$?,echo Running \"$(example)\" example; ./$(example); echo;)

.SILENT: clean
.PHONY: clean
clean:
	$(foreach file,$(ALL),rm -f $(file).o $(file))
