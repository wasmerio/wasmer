#!/usr/bin/env bash
# Source this before configure. Derive each cache value by linking a volatile
# pointer with the installed header's real type, avoiding K&R trap stubs.
set -euo pipefail
mkdir -p "$ROOT/openssl-probes"
for function in BN_is_prime_ex DES_crypt DSA_generate_parameters_ex \
  EVP_DigestSign EVP_DigestVerify EVP_DigestFinal_ex EVP_DigestInit_ex \
  EVP_MD_CTX_cleanup EVP_MD_CTX_copy_ex EVP_MD_CTX_init HMAC_CTX_init \
  RSA_generate_key_ex RSA_get_default_method EVP_CIPHER_CTX_iv \
  EVP_CIPHER_CTX_iv_noconst EVP_CIPHER_CTX_get_iv EVP_CIPHER_CTX_get_updated_iv \
  EVP_CIPHER_CTX_set_iv OpenSSL_version OpenSSL_version_num EC_KEY_METHOD_new \
  EVP_PKEY_get_raw_private_key EVP_PKEY_get_raw_public_key; do
  cat > "$ROOT/openssl-probes/$function.c" <<C
#include <openssl/bn.h>
#include <openssl/crypto.h>
#include <openssl/des.h>
#include <openssl/dsa.h>
#include <openssl/ec.h>
#include <openssl/evp.h>
#include <openssl/hmac.h>
#include <openssl/rsa.h>
static __typeof__(&$function) volatile symbol = &$function;
int main(void) { return symbol == 0; }
C
  if "$CC" -O2 -I"$PREFIX/include" "$ROOT/openssl-probes/$function.c" \
      -L"$PREFIX/lib" -lcrypto -o "$ROOT/openssl-probes/$function.wasm" \
      > "$ROOT/openssl-probes/$function.log" 2>&1; then
    export "ac_cv_func_$function=yes"
    echo "typed OpenSSL probe $function yes"
  else
    export "ac_cv_func_$function=no"
    echo "typed OpenSSL probe $function no (see openssl-probes/$function.log)"
  fi
done
