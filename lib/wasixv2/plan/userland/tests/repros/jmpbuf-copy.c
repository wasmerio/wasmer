/* bash's pattern (execute_function + eval's parse_and_execute): save a jmp_buf
 * by copying it, setjmp the same buffer in a nested frame, restore the copy on
 * return, then longjmp to the outer context. Valid C: the outer setjmp frame is
 * still live. Expected output: "ok". */
#include <setjmp.h>
#include <stdio.h>
#include <string.h>
static jmp_buf jb;
static void nested(void) {
  jmp_buf saved;
  memcpy(saved, jb, sizeof jb);
  if (setjmp(jb) == 0) { /* nothing jumps here */ }
  memcpy(jb, saved, sizeof jb);
}
int main(void) {
  if (setjmp(jb)) { puts("ok"); return 0; }
  nested();
  longjmp(jb, 1);
}
