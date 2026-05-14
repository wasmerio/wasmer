//#ExpectedStdout: error number: 444
#include <stdio.h>

extern int erryes;

int main() {
  printf("error number: %i\n", erryes);
  return 0;
}
