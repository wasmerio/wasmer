#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

wasix_context_id_t context1;
wasix_context_id_t context2;

char *message = "Uninitialized\n";
int stop = 0;
int counter = 0;

void test1(void) {
  while (1) {
    wasix_context_switch(context2);
    if (stop == 1) {
      wasix_context_switch(wasix_context_main);
    }
    counter++;
    printf("%s", message);
  }
}

void test2(void) {
  printf("Starting test2\n");

  message = "Switch 1\n";
  wasix_context_switch(context1);

  message = "Switch 2\n";
  wasix_context_switch(context1);

  message = "Switch 3\n";
  wasix_context_switch(context1);

  message = "Switch 4\n";
  wasix_context_switch(context1);

  stop = 1;
  wasix_context_switch(context1);

  exit(1);
}

int main() {
  wasix_context_create(&context1, test1);
  wasix_context_create(&context2, test2);
  wasix_context_switch(context1);

  assert(counter == 4);

  return 0;
}