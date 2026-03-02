#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>

typedef int (* side_type)();

int main() {
  void *handle = dlopen("libside.so", RTLD_NOW);
  if (!handle) {
    printf("dlopen failed: %s\n", dlerror());
    return 1;
  }
  side_type side = (side_type)dlsym(handle, "side");
  if (!side) {
    printf("dlsym failed: %s\n", dlerror());
    return 1;
  }
  side();
  exit(0);
}
