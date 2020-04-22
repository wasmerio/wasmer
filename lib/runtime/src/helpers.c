#include <setjmp.h>

int RegisterSetjmp(
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

void Unwind(void *JmpBuf) {
  jmp_buf *buf = (jmp_buf*) JmpBuf;
  longjmp(*buf, 1);
}


#ifdef __APPLE__
#include <sys/ucontext.h>

void* GetPcFromUContext(ucontext_t *cx) {
  return (void*) cx->uc_mcontext->__ss.__rip;
}
#endif
