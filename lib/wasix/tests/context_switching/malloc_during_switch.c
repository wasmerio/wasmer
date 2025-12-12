// Test memory allocations during context switches
// This can trigger store context issues if malloc implementation
// does host calls while the store is borrowed
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wasix/context.h>

wasix_context_id_t ctx1, ctx2, ctx3;
void *allocations[100];
int alloc_count = 0;

void allocate_and_switch(void);

void context1_fn(void) {
  allocate_and_switch();

  // Free some allocations
  for (int i = 0; i < 10 && i < alloc_count; i++) {
    free(allocations[i]);
    allocations[i] = NULL;
  }

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  allocate_and_switch();

  // Reallocate some memory
  for (int i = 10; i < 20 && i < alloc_count; i++) {
    if (allocations[i]) {
      allocations[i] = realloc(allocations[i], 2048);
      assert(allocations[i] != NULL);
    }
  }

  wasix_context_switch(ctx1);
}

void context3_fn(void) {
  allocate_and_switch();

  // More allocations
  for (int i = 0; i < 15; i++) {
    void *ptr = calloc(1, 512);
    assert(ptr != NULL);
    if (alloc_count < 100) {
      allocations[alloc_count++] = ptr;
    }
  }

  wasix_context_switch(ctx2);
}

void allocate_and_switch(void) {
  // Allocate memory
  for (int i = 0; i < 10; i++) {
    void *ptr = malloc(1024 + i * 100);
    assert(ptr != NULL);
    memset(ptr, i, 1024 + i * 100);

    if (alloc_count < 100) {
      allocations[alloc_count++] = ptr;
    }

    // Switch contexts during allocations
    if (i == 5) {
      if (alloc_count < 30) {
        wasix_context_switch(ctx3);
      } else if (alloc_count < 60) {
        wasix_context_switch(ctx2);
      }
    }
  }
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  ret = wasix_context_create(&ctx3, context3_fn);
  assert(ret == 0 && "Failed to create context 3");

  // Start execution
  wasix_context_switch(ctx1);

  // Free remaining allocations
  for (int i = 0; i < alloc_count; i++) {
    if (allocations[i]) {
      free(allocations[i]);
    }
  }

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);
  wasix_context_destroy(ctx3);

  fprintf(stderr, "Malloc during switch test passed\n");
  return 0;
}
