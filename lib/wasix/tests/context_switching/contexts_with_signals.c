#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <wasix/context.h>

// Test context switching with signal handlers

wasix_context_id_t ctx1, ctx2;
volatile int signal_received = 0;

void signal_handler(int sig) {
  signal_received++;
  fprintf(stderr, "Signal %d received (count=%d)\n", sig, signal_received);
}

void context1_fn(void) {
  // Install signal handler
  signal(SIGUSR1, signal_handler);

  // Raise a signal
  raise(SIGUSR1);
  assert(signal_received == 1 && "Signal should have been received");

  // Switch to context 2
  wasix_context_switch(ctx2);

  // After resuming, check if context 2's signal was received
  assert(signal_received == 2 &&
         "Signal from context 2 should have been received");

  wasix_context_switch(wasix_context_main);
}

void context2_fn(void) {
  // The signal handler should still be installed
  // (signal handlers are per-process, not per-context)

  // Raise another signal
  raise(SIGUSR1);
  assert(signal_received == 2 && "Second signal should have been received");

  // Switch back to context 1
  wasix_context_switch(ctx1);
}

int main() {
  int ret;

  ret = wasix_context_create(&ctx1, context1_fn);
  assert(ret == 0 && "Failed to create context 1");

  ret = wasix_context_create(&ctx2, context2_fn);
  assert(ret == 0 && "Failed to create context 2");

  // Start execution
  wasix_context_switch(ctx1);

  // Verify both signals were received
  assert(signal_received == 2 && "Total of 2 signals should be received");

  // Cleanup
  wasix_context_destroy(ctx1);
  wasix_context_destroy(ctx2);

  fprintf(stderr, "Signal handling switching test passed\n");
  return 0;
}
