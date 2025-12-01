#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

wasix_context_id_t context1;
wasix_context_id_t context2;

int stop = 0;
int counter = 0;

void test1(void) {
  while (1) {
    wasix_context_switch(wasix_context_main);
    if (stop == 1) {
      wasix_context_switch(wasix_context_main);
      break;
    }
    counter++;
  }
}

int main() {
  wasix_context_create(&context1, test1);
  wasix_context_switch(context1);

  wasix_context_switch(context1);
  wasix_context_switch(context1);
  wasix_context_switch(context1);
  wasix_context_switch(context1);
  stop = 1;
  wasix_context_switch(context1);

  assert(counter == 4);
  return 0;
}