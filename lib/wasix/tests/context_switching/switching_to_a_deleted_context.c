#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

wasix_context_id_t context1;
wasix_context_id_t context2;

int counter = 0;

void test1(void) {
  counter += 1;
  wasix_context_switch(context_main_context);
}

// Required because switching with the real main currently segfaults
void test_main(void) {
  wasix_context_create(&context1, test1);
  wasix_context_create(&context2, test1);

  // Assert that test1 increments the context.
  assert(counter == 0);
  wasix_context_switch(context1);
  assert(counter == 1);

  // Assert that calling the destroyed context again fails
  wasix_context_destroy(context1);
  wasix_context_switch(context1);
  assert(counter == 1);

  // Assert that switching to a context that was destroyed before the first
  // switch works
  wasix_context_destroy(context2);
  wasix_context_switch(context2);
  assert(counter == 1);

  exit(0);
}

int main() {
  wasix_context_id_t test_main_context;
  wasix_context_create(&test_main_context, test_main);
  wasix_context_switch(test_main_context);

  return 0;
}