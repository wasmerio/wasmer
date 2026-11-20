//#UnixOnly: true
//#SkipEngine:V8:async functions are not supported yet
//#MustFail: true
//#ExpectedStdout: back on the main context

// A guest fault on one context while another is suspended must end the guest
// with a trap. The trap handler state of the two contexts used to be mixed
// up, and the fault killed the host process instead.

#include <assert.h>
#include <stdio.h>
#include <wasix/context.h>

wasix_context_id_t other;

void entry(void) { wasix_context_switch(wasix_context_main); }

int main() {
  int ret = wasix_context_create(&other, entry);
  assert(ret == 0);
  ret = wasix_context_switch(other);
  assert(ret == 0);
  printf("back on the main context\n");
  fflush(stdout);
  volatile int* out_of_bounds = (int*)0xfffffff0;
  return *out_of_bounds;
}
