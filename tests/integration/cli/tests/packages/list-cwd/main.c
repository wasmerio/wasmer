#include <dirent.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char* argv[]) {
  DIR* dir;
  struct dirent* entry;

  dir = opendir("./");
  if (dir == NULL) {
    perror("opendir");
    return 1;
  }

  while ((entry = readdir(dir)) != NULL) {
    printf("%s\n", entry->d_name);
  }

  closedir(dir);

  return 0;
}
