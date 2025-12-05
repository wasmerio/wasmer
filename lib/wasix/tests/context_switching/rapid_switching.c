#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <wasix/context.h>

// Test rapid context switching with many iterations

#define SWITCH_COUNT 1000

wasix_context_id_t ctx1, ctx2;
int ping_count = 0;
int pong_count = 0;

void ping_context(void) {
  while (ping_count < SWITCH_COUNT) {
    ping_count++;
    wasix_context_switch(ctx2);
  }
  wasix_context_switch(wasix_context_main);
}

void pong_context(void) {
  while (pong_count < SWITCH_COUNT) {
    pong_count++;
    wasix_context_switch(ctx1);
  }
  wasix_context_switch(wasix_context_main);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, ping_context);
  assert(ret == 0 && "Failed to create ping context");

  ret = wasix_context_create(&ctx2, pong_context);
  assert(ret == 0 && "Failed to create pong context");

  // Start the ping-pong
  wasix_context_switch(ctx1);

  // Verify all switches occurred
  assert(ping_count == SWITCH_COUNT && "Ping count mismatch");
  assert(pong_count == SWITCH_COUNT && "Pong count mismatch");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Rapid switching test passed (%d switches)\n",
          SWITCH_COUNT * 2);
  return 0;
}
