#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

#ifndef NULL
#define NULL ((void *)0)
#endif

wasix_context_id_t context1;
wasix_context_id_t context2;

char *message = "Uninitialized\n";
int stop = 0;
int counter = 0;

void test1(void) {
  while (1) {
    wasix_context_switch(context2);
    if (stop == 1) {
      wasix_context_switch(context_main_context);
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

  exit(50);
}

void *abort_in_thread(void *arg) {
  wasix_context_create(&context1, test1);
  wasix_context_create(&context2, test2);
  wasix_context_switch(context1);

  return NULL;
}

int main() {
  pthread_t thread;
  pthread_create(&thread, NULL, abort_in_thread, NULL);
  pthread_join(thread, NULL);

  if (counter != 4) {
    printf("Error: expected counter to be 4 but it is %d\n", counter);
    exit(1);
  }

  return 0;
}