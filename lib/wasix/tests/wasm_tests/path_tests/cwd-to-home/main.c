#include <fcntl.h>
#include <stdio.h>

int main() {
  int fd = open("hello.txt", O_RDWR);
  printf("%d", (fd == -1));

  return 0;
}