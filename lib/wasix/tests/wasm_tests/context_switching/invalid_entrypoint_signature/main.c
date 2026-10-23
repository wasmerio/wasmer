//#UnixOnly: true
//#SkipEngine:V8:async functions are not supported yet

// A context entrypoint must take nothing and return nothing. One that takes an
// argument, or returns a value, is refused when the context is created rather
// than failing later, when the context is first switched to.

#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <wasix/context.h>

typedef void (*entrypoint_t)(void);

void takes_an_argument(int value) { (void)value; }

int returns_a_value(void) { return 1; }

void valid(void) { wasix_context_switch(wasix_context_main); }

int main() {
  wasix_context_id_t context;

  errno = 0;
  int ret = wasix_context_create(&context, (entrypoint_t)takes_an_argument);
  assert(ret == -1 && "an entrypoint taking an argument should be refused");
  assert(errno == EINVAL);

  errno = 0;
  ret = wasix_context_create(&context, (entrypoint_t)returns_a_value);
  assert(ret == -1 && "an entrypoint returning a value should be refused");
  assert(errno == EINVAL);

  // A valid one is still accepted.
  ret = wasix_context_create(&context, valid);
  assert(ret == 0 && "a valid entrypoint should be accepted");
  wasix_context_switch(context);

  printf("ok\n");
  return 0;
}
