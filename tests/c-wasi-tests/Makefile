all: install

CC := clang-19
LD := wasm-ld-19
WASIX_SYSROOT_PIC ?= $(abspath $(CURDIR)/../../../wasix-libc/sysroot-pic)
WASMER ?= $(CURDIR)/../../target/release/wasmer

WASI_COMMON_CFLAGS := \
  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
  -matomics -mbulk-memory -mmutable-globals -pthread \
  -mthread-model posix -ftls-model=local-exec \
  -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
  -D_WASI_EMULATED_PROCESS_CLOCKS \
  -g -flto -O0

WASI_WASM_CFLAGS := $(WASI_COMMON_CFLAGS) -fPIC
WASI_SO_CFLAGS := $(WASI_COMMON_CFLAGS) -fPIC -fvisibility=default

WASI_WASM_LD_PREFIX := \
  -L${WASIX_SYSROOT_PIC}/lib \
  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
  --whole-archive --export-all

WASI_WASM_LD_SUFFIX := \
  ${WASIX_SYSROOT_PIC}/lib/wasm32-wasi/crt1.o \
  -lc -lresolv -lrt -lm -lpthread \
  -lwasi-emulated-mman \
  ${WASIX_SYSROOT_PIC}/lib/wasm32-wasi/libclang_rt.builtins-wasm32.a \
  --max-memory=4294967296 --import-memory --shared-memory \
  --extra-features=atomics,bulk-memory,mutable-globals \
  --export=__wasm_signal --export=__tls_size --export=__tls_align \
  --export=__tls_base --export=__wasm_call_ctors --export-if-defined=__wasm_apply_data_relocs \
  --experimental-pic \
  --allow-undefined \
  -pie \
  --no-export-dynamic

WASI_SO_LDFLAGS := \
  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
  --extra-features=atomics,bulk-memory,mutable-globals \
  --export=__wasm_call_ctors --export-if-defined=__wasm_apply_data_relocs \
  --experimental-pic --unresolved-symbols=import-dynamic \
  -shared --shared-memory \
  ${WASIX_SYSROOT_PIC}/lib/wasm32-wasi/libclang_rt.builtins-wasm32.a

define build_wasm
	$(CC) $(WASI_WASM_CFLAGS) -c $< -o $(patsubst %.wasm, %.o, $@)
	$(LD) $(1) $(WASI_WASM_LD_PREFIX) $(patsubst %.wasm, %.o, $@) $(WASI_WASM_LD_SUFFIX) -o $@
endef

define build_shared
	$(CC) $(WASI_SO_CFLAGS) -c $< -o $(patsubst %.so, %.o, $@)
	$(LD) $(1) $(WASI_SO_LDFLAGS) -o $@ $(patsubst %.so, %.o, $@)
endef

TESTS := \
  callback_signal \
  chdir \
  clock_time_get \
  clock_time_set \
  closure_allocate \
  closure_free \
  closure_prepare \
  context_destroy \
  dlopen \
  dylink-needed \
  epoll_create \
  fd_advise \
  fd_allocate \
  fd_close \
  fd_datasync \
  fd_dup \
  fd_dup2 \
  fd_fdstat_get \
  fd_fdstat_set_rights \
  fd_fdstat_set_flags \
  fd_fdflags_get \
  fd_fdflags_set \
  fd_event \
  fd_write \
  fd_read \
  fd_filestat_set_size \
  fd_filestat_set_times \
  fd_filestat_get \
  fd_seek \
  fd_renumber \
  fd_sync \
  fd_tell \
  fd_pipe \
  getcwd \
  path_create_directory \
  path_filestat_get \
  path_filestat_set_times \
  path_link \
  path_readlink \
  path_remove_directory \
  path_symlink \
  path_unlink_file \
  proc_id \
  proc_parent \
  proc_signals_sizes_get \
  proc_signals_get \
  proc_exit_code_1 \
  proc_exit_code_2 \
  proc_exit_code_123 \
  proc_exit_code_125 \
  proc_exit_code_126 \
  proc_exit_hello \
  proc_exit_fpr \
  proc_exit2__exit_65 \
  proc_exit2_exit_65 \
  proc_raise \
  port_addr_list \
  port_addr_clear \
  port_addr_remove \
  port_route_list \
  port_route_clear \
  port_mac \
  reflect_signature \
  random_get \
  sched_yield \
  sock_addr_peer \
  sock_bind \
  sock_get_opt_flag \
  sock_get_opt_size \
  sock_get_opt_time \
  sock_leave_multicast_v4 \
  sock_leave_multicast_v6 \
  sock_listen \
  sock_open \
  sock_recv \
  sock_send \
  sock_shutdown \
  sock_status \
  thread_exit \
  thread_signal \
  thread_sleep \
  thread_id \
  thread_parallelism \
  tty_set \
  tty_get

BUILD_TARGETS := libside1.so libside2.so $(addsuffix .wasm,$(TESTS))

libside1.so: side1.c libside2.so | check-sysroot
	$(call build_shared,-L. -lside2)

libside2.so: side2.c | check-sysroot
	$(call build_shared,)

dylink-needed.wasm: dylink-needed.c libside1.so
	$(call build_wasm,-L. -lside1)

%.wasm: %.c | check-sysroot
	$(call build_wasm,)

%.wat: %.wasm
	wasm-tools print $< > $@

%.wat: %.so
	wasm-tools print $< > $@

run-getcwd: getcwd.wasm
	../../target/release/wasmer run --dir=. --enable-threads getcwd.wasm

run: $(BUILD_TARGETS)
	@if [ ! -x "$(WASMER)" ]; then \
		echo "WASMER not found: $(WASMER)" >&2; \
		exit 1; \
	fi
	@failures=0; \
	for t in $(TESTS); do \
		expected=0; \
		case "$$t" in \
			proc_exit_code_1) expected=1 ;; \
			proc_exit_code_2) expected=2 ;; \
			proc_exit_code_123) expected=123 ;; \
			proc_exit_code_125) expected=125 ;; \
			proc_exit_code_126) expected=126 ;; \
			proc_exit_hello) expected=1 ;; \
			proc_exit2__exit_65) expected=65 ;; \
			proc_exit2_exit_65) expected=65 ;; \
		esac; \
		flags="--enable-threads --dir=. --dir=/dev"; \
		case "$$t" in \
			dlopen|dylink-needed) flags="$$flags --mapdir /lib:$(CURDIR)" ;; \
			fd_close|fd_filestat_set_size|port_addr_list|port_addr_clear|port_addr_remove|port_route_list|port_route_clear|port_mac|sock_addr_peer|sock_bind|sock_get_opt_flag|sock_get_opt_size|sock_get_opt_time|sock_leave_multicast_v4|sock_leave_multicast_v6|sock_listen|sock_open|sock_recv|sock_send|sock_shutdown|sock_status) flags="$$flags --net" ;; \
		esac; \
		"$(WASMER)" run $$flags "$$t.wasm"; rc=$$?; \
		if [ $$rc -ne $$expected ]; then \
			echo "FAIL $$t: expected $$expected, got $$rc" >&2; \
			failures=$$((failures + 1)); \
		else \
			echo "PASS $$t"; \
		fi; \
	done; \
	if [ $$failures -ne 0 ]; then \
		echo "$$failures test(s) failed" >&2; \
		exit 1; \
	fi

install: libside1.so libside2.so dlopen.wasm dylink-needed.wasm proc_id.wasm thread_id.wasm fd_tell.wasm random_get.wasm sched_yield.wasm closure_free.wasm fd_fdstat_get.wasm fd_fdflags_get.wasm getcwd.wasm
	cp -f $^ ../integration/cli/tests/wasm

clean:
	rm -f *.wasm *.wat *.o *.so

.PHONY: check-sysroot
check-sysroot:
	@if [ ! -f "$(WASIX_SYSROOT_PIC)/include/assert.h" ]; then \
		echo "WASIX_SYSROOT_PIC does not look valid: $(WASIX_SYSROOT_PIC)" >&2; \
		echo "Expected header: $(WASIX_SYSROOT_PIC)/include/assert.h" >&2; \
		exit 1; \
	fi
