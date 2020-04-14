###############################################################################
# Configuration

# Change these as you see fit.

V8_VERSION = branch-heads/7.6
V8_ARCH = x64
V8_MODE = release

WASM_FLAGS = -DWASM_API_DEBUG  # -DWASM_API_DEBUG_LOG
C_FLAGS = ${WASM_FLAGS} -Wall -Werror -ggdb -O -fsanitize=address
CC_FLAGS = -std=c++11 ${C_FLAGS}
LD_FLAGS = -fsanitize-memory-track-origins -fsanitize-memory-use-after-dtor

C_COMP = clang

WASM_INTERPRETER = ../spec/interpreter/wasm  # Adjust as needed.

# No need to change what follows.

# Base directories
V8_DIR = v8
WASM_DIR = .
EXAMPLE_DIR = example
OUT_DIR = out

# Example config
EXAMPLE_OUT = ${OUT_DIR}/${EXAMPLE_DIR}
EXAMPLES = \
  hello \
  callback \
  trap \
  start \
  reflect \
  global \
  table \
  memory \
  hostref \
  finalize \
  serialize \
  threads \
  # multi \

# Wasm config
WASM_INCLUDE = ${WASM_DIR}/include
WASM_SRC = ${WASM_DIR}/src
WASM_OUT = ${OUT_DIR}/${WASM_DIR}
WASM_C_LIBS = wasm-bin wasm-c
WASM_CC_LIBS = wasm-bin wasm-v8
WASM_C_O = ${WASM_C_LIBS:%=${WASM_OUT}/%.o}
WASM_CC_O = ${WASM_CC_LIBS:%=${WASM_OUT}/%.o}
WASM_V8_PATCH = wasm-v8-lowlevel

# V8 config
V8_BUILD = ${V8_ARCH}.${V8_MODE}
V8_V8 = ${V8_DIR}/v8
V8_DEPOT_TOOLS = ${V8_DIR}/depot_tools
V8_PATH = $(abspath ${V8_DEPOT_TOOLS}):${PATH}
V8_INCLUDE = ${V8_V8}/include
V8_SRC = ${V8_V8}/src
V8_OUT = ${V8_V8}/out.gn/${V8_BUILD}
V8_LIBS = monolith # base libbase external_snapshot libplatform libsampler
V8_BLOBS = # natives_blob snapshot_blob
V8_CURRENT = $(shell if [ -f ${V8_OUT}/version ]; then cat ${V8_OUT}/version; else echo ${V8_VERSION}; fi)

V8_GN_ARGS = \
  is_component_build=false \
  v8_static_library=true \
  v8_monolithic=true \
  v8_use_external_startup_data=false \
  v8_enable_i18n_support=false \
  use_custom_libcxx=false \
  use_custom_libcxx_for_host=false

# Compiler config
ifeq (${C_COMP},clang)
  CC_COMP = clang++
  LD_GROUP_START = 
  LD_GROUP_END = 
else ifeq (${C_COMP},gcc)
  CC_COMP = g++
  LD_GROUP_START = -Wl,--start-group
  LD_GROUP_END = -Wl,--end-group
else
  $(error C_COMP set to unknown compiler, must be clang or gcc)
endif


###############################################################################
# Examples
#
# To build Wasm APIs and run all examples:
#   make all
#
# To run only C examples:
#   make c
#
# To run only C++ examples:
#   make cc
#
# To run individual C example (e.g. hello):
#   make run-hello-c
#
# To run individual C++ example (e.g. hello):
#   make run-hello-cc
#
# To rebuild after V8 version change:
#   make clean all

.PHONY: all cc c
all: cc c
cc: ${EXAMPLES:%=run-%-cc}
c: ${EXAMPLES:%=run-%-c}

# Running a C / C++ example
run-%-c: ${EXAMPLE_OUT}/%-c ${EXAMPLE_OUT}/%.wasm ${V8_BLOBS:%=${EXAMPLE_OUT}/%.bin}
	@echo ==== C ${@:run-%-c=%} ====; \
	cd ${EXAMPLE_OUT}; ./${@:run-%=%}
	@echo ==== Done ====

run-%-cc: ${EXAMPLE_OUT}/%-cc ${EXAMPLE_OUT}/%.wasm ${V8_BLOBS:%=${EXAMPLE_OUT}/%.bin}
	@echo ==== C++ ${@:run-%-cc=%} ====; \
	cd ${EXAMPLE_OUT}; ./${@:run-%=%}
	@echo ==== Done ====

# Compiling C / C++ example
${EXAMPLE_OUT}/%-c.o: ${EXAMPLE_DIR}/%.c ${WASM_INCLUDE}/wasm.h
	mkdir -p ${EXAMPLE_OUT}
	${C_COMP} -c ${C_FLAGS} -I. -I${V8_INCLUDE} -I${WASM_INCLUDE} $< -o $@

${EXAMPLE_OUT}/%-cc.o: ${EXAMPLE_DIR}/%.cc ${WASM_INCLUDE}/wasm.hh
	mkdir -p ${EXAMPLE_OUT}
	${CC_COMP} -c ${CC_FLAGS} -I. -I${V8_INCLUDE} -I${WASM_INCLUDE} $< -o $@

# Linking C / C++ example
.PRECIOUS: ${EXAMPLES:%=${EXAMPLE_OUT}/%-c}
${EXAMPLE_OUT}/%-c: ${EXAMPLE_OUT}/%-c.o ${WASM_C_O}
	${CC_COMP} ${CC_FLAGS} ${LD_FLAGS} $< -o $@ \
		${WASM_C_O} \
		${LD_GROUP_START} \
		${V8_LIBS:%=${V8_OUT}/obj/libv8_%.a} \
		${LD_GROUP_END} \
		-ldl -pthread

.PRECIOUS: ${EXAMPLES:%=${EXAMPLE_OUT}/%-cc}
${EXAMPLE_OUT}/%-cc: ${EXAMPLE_OUT}/%-cc.o ${WASM_CC_O}
	${CC_COMP} ${CC_FLAGS} ${LD_FLAGS} $< -o $@ \
		${WASM_CC_O} \
		${LD_GROUP_START} \
		${V8_LIBS:%=${V8_OUT}/obj/libv8_%.a} \
		${LD_GROUP_END} \
		-ldl -pthread

# Installing V8 snapshots
.PRECIOUS: ${V8_BLOBS:%=${EXAMPLE_OUT}/%.bin}
${EXAMPLE_OUT}/%.bin: ${V8_OUT}/%.bin
	cp $< $@

# Installing Wasm binaries
.PRECIOUS: ${EXAMPLES:%=${EXAMPLE_OUT}/%.wasm}
${EXAMPLE_OUT}/%.wasm: ${EXAMPLE_DIR}/%.wasm
	cp $< $@

# Assembling Wasm binaries
.PRECIOUS: %.wasm
%.wasm: %.wat
	${WASM_INTERPRETER} -d $< -o $@


###############################################################################
# Wasm C / C++ API
#
# To build both C / C++ APIs:
#   make wasm

.PHONY: wasm wasm-c wasm-cc
wasm: wasm-c wasm-cc
wasm-c: ${WASM_C_LIBS:%=${WASM_OUT}/%.o}
wasm-cc: ${WASM_CC_LIBS:%=${WASM_OUT}/%.o}


# Compiling
${WASM_OUT}/%.o: ${WASM_SRC}/%.cc ${WASM_INCLUDE}/wasm.h ${WASM_INCLUDE}/wasm.hh
	mkdir -p ${WASM_OUT}
	${CC_COMP} -c ${CC_FLAGS} -I. -I${V8_INCLUDE} -I${WASM_INCLUDE} -I${WASM_SRC} $< -o $@

# wasm-c.cc includes wasm-v8.cc, so set up a side dependency
${WASM_OUT}/wasm-c.o: ${WASM_SRC}/wasm-v8.cc


###############################################################################
# Clean-up

.PHONY: clean
clean:
	rm -rf ${OUT_DIR}


###############################################################################
# V8
#
# To get and build V8:
#   make v8-checkout
#   make v8
#
# To update and build current branch:
#   make v8-update
#   make v8

# Building

.PHONY: v8
v8: ${V8_INCLUDE}/${WASM_V8_PATCH}.hh ${V8_SRC}/${WASM_V8_PATCH}.cc v8-patch v8-build v8-unpatch

.PHONY: v8-build
v8-build:
	@echo ==== Building V8 ${V8_CURRENT} ${V8_BUILD} ====
	(cd ${V8_V8}; PATH=${V8_PATH} tools/dev/v8gen.py ${V8_BUILD} -- ${V8_GN_ARGS})
	(cd ${V8_V8}; PATH=${V8_PATH} ninja -C out.gn/${V8_BUILD})
	(cd ${V8_V8}; touch out.gn/${V8_BUILD}/args.gn)
	(cd ${V8_V8}; PATH=${V8_PATH} ninja -C out.gn/${V8_BUILD})

.PHONY: v8-patch
v8-patch:
	if ! grep ${WASM_V8_PATCH} ${V8_V8}/BUILD.gn; then \
	  cp ${V8_V8}/BUILD.gn ${V8_V8}/BUILD.gn.save; \
	  cd ${V8_V8}; \
	  patch < ../../patch/0001-BUILD.gn-add-wasm-v8-lowlevel.patch; \
	fi

.PHONY: v8-unpatch
v8-unpatch:
	if [ -f ${V8_V8}/BUILD.gn.save ]; then \
	  mv -f ${V8_V8}/BUILD.gn.save ${V8_V8}/BUILD.gn; \
	fi

${V8_INCLUDE}/${WASM_V8_PATCH}.hh: ${WASM_SRC}/${WASM_V8_PATCH}.hh
	cp $< $@

${V8_SRC}/${WASM_V8_PATCH}.cc: ${WASM_SRC}/${WASM_V8_PATCH}.cc
	cp $< $@


# Check-out

# Check out set version
.PHONY: v8-checkout
v8-checkout: v8-checkout-banner ${V8_DEPOT_TOOLS} ${V8_V8}
	(cd ${V8_V8}; git checkout -f master)
	(cd ${V8_V8}; git pull)
	(cd ${V8_V8}; git checkout ${V8_VERSION})
	(cd ${V8_V8}; PATH=${V8_PATH} gclient sync)
	mkdir -p ${V8_OUT}
	echo >${V8_OUT}/version ${V8_VERSION}
	@if [ ${V8_CURRENT} != ${V8_VERSION} ]; then echo ==== Done. If you have trouble building V8, run \`make v8-clean\` first ====; fi

.PHONY: v8-checkout-banner
v8-checkout-banner:
	@echo ==== Checking out V8 ${V8_VERSION} ====

${V8_DEPOT_TOOLS}:
	mkdir -p ${V8_DIR}
	(cd ${V8_DIR}; git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git)

${V8_V8}:
	mkdir -p ${V8_DIR}
	(cd ${V8_DIR}; PATH=${V8_PATH} fetch v8)
	(cd ${V8_V8}; git checkout ${V8_VERSION})

# Update current check-out
.PHONY: v8-update
v8-update: v8-unpatch
	@echo ==== Updating V8 ${V8_CURRENT} ====
	(cd ${V8_V8}; git pull origin ${V8_CURRENT})
	(cd ${V8_V8}; PATH=${V8_PATH} gclient sync)


# Clean-up

# Delete V8 build
.PHONY: v8-clean
v8-clean:
	rm -rf ${V8_OUT}
	mkdir -p ${V8_OUT}
	echo >${V8_OUT}/version ${V8_VERSION}


# Show current V8 version
.PHONY: v8-version
v8-version:
	@echo Checked out V8 version: ${V8_CURRENT}


# Display V8 build configuration
.PHONY: v8-gn-args
v8-gn-args:
	@echo ${V8_GN_ARGS}


###############################################################################
# Docker
#

.PHONY: docker
docker:
	docker build -t wasm:Dockerfile .
