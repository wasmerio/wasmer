// This file contains partial code from other sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

#include <setjmp.h>

int register_setjmp(
    void **buf_storage,
    void (*body)(void*),
    void *payload) {
  jmp_buf buf;
  if (setjmp(buf) != 0) {
    return 0;
  }
  *buf_storage = &buf;
  body(payload);
  return 1;
}

void unwind(void *JmpBuf) {
  jmp_buf *buf = (jmp_buf*) JmpBuf;
  longjmp(*buf, 1);
}
