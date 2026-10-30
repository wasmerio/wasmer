#!/usr/bin/env bash
# Same actual-header volatile-symbol pattern as openssl-probes-9.sh.
mkdir -p "$ROOT/curl-openssl-probes"
for function in HMAC_Update HMAC_Init_ex SSL_connect; do
  cat > "$ROOT/curl-openssl-probes/$function.c" <<C
#include <openssl/hmac.h>
#include <openssl/ssl.h>
static __typeof__(&$function) volatile symbol = &$function;
int main(void) { return symbol == 0; }
C
  "$CC" -O2 -I"$PREFIX/include" "$ROOT/curl-openssl-probes/$function.c" -L"$PREFIX/lib" -lssl -lcrypto -o "$ROOT/curl-openssl-probes/$function.wasm"
  "$WASIXV2_PROBE_CLI" run --compiler-threads 3 --no-tty "$ROOT/curl-openssl-probes/$function.wasm"
  if [ "$function" = SSL_connect ]; then library=ssl; else library=crypto; fi
  export "ac_cv_lib_${library}_${function}=yes"
done
