#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static void handle_alarm(int sig) { _exit(0); }

int main(void) {
  signal(SIGALRM, handle_alarm);
  alarm(1);
  printf("Calling sleep\n");
  sleep(2);
  printf("Calling sleep\n");
  sleep(2);

  // We waited long enough and should have called handle_alarm by now
  // If we reach this point, the SIGALRM got lost.
  return 1;
}
