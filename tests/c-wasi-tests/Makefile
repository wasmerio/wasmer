all: install

libside1.so: side1.c libside2.so
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC -fvisibility=default \
      -c $< -o $(patsubst %.so, %.o, $@)

	wasm-ld-19 \
	  -L. -lside2 \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --extra-features=atomics,bulk-memory,mutable-globals \
	  --export=__wasm_call_ctors --export-if-defined=__wasm_apply_data_relocs \
	  --experimental-pic --unresolved-symbols=import-dynamic \
	  -shared --shared-memory \
	  ${WASIX_SYSROOT_PIC}/lib/wasm32-wasi/libclang_rt.builtins-wasm32.a \
	  -o $@ $(patsubst %.so, %.o, $@)

libside2.so: side2.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC -fvisibility=default \
      -c $< -o $(patsubst %.so, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --extra-features=atomics,bulk-memory,mutable-globals \
	  --export=__wasm_call_ctors --export-if-defined=__wasm_apply_data_relocs \
	  --experimental-pic --unresolved-symbols=import-dynamic \
	  -shared --shared-memory \
	  ${WASIX_SYSROOT_PIC}/lib/wasm32-wasi/libclang_rt.builtins-wasm32.a \
	  -o $@ $(patsubst %.so, %.o, $@)

dlopen.wasm: dlopen.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

dylink-needed.wasm: dylink-needed.c libside1.so
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L. -lside1 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

%.wat: %.wasm
	wasm-tools print $< > $@

%.wat: %.so
	wasm-tools print $< > $@

proc_id.wasm: proc_id.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

thread_id.wasm: thread_id.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

fd_tell.wasm: fd_tell.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

random_get.wasm: random_get.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

getcwd.wasm: getcwd.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

run-getcwd: getcwd.wasm
	../../target/release/wasmer run --dir=. --enable-threads getcwd.wasm

install: libside1.so libside2.so dlopen.wasm dylink-needed.wasm proc_id.wasm thread_id.wasm fd_tell.wasm random_get.wasm sched_yield.wasm closure_free.wasm fd_fdstat_get.wasm fd_fdflags_get.wasm getcwd.wasm
	cp -f $^ ../integration/cli/tests/wasm

clean:
	rm -f *.wasm *.wat *.o *.so
sched_yield.wasm: sched_yield.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

closure_free.wasm: closure_free.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

fd_fdstat_get.wasm: fd_fdstat_get.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)

	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@

fd_fdflags_get.wasm: fd_fdflags_get.c
	clang-19 \
	  --target=wasm32-wasi --sysroot=${WASIX_SYSROOT_PIC} \
	  -matomics -mbulk-memory -mmutable-globals -pthread \
	  -mthread-model posix -ftls-model=local-exec \
      -fno-trapping-math -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL \
	  -D_WASI_EMULATED_PROCESS_CLOCKS \
      -g -flto -O0 \
	  -fPIC \
      -c $< -o $(patsubst %.wasm, %.o, $@)
	wasm-ld-19 \
	  -L${WASIX_SYSROOT_PIC}/lib \
	  -L${WASIX_SYSROOT_PIC}/lib/wasm32-wasi \
	  --whole-archive --export-all \
	  $(patsubst %.wasm, %.o, $@) \
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
	  --no-export-dynamic \
	  -o $@
