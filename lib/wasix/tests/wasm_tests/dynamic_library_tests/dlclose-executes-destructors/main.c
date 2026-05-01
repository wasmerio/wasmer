#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>

__attribute__((constructor)) static void init() { printf("a"); }
// The main module is loaded first, so its constructor runs before `main`.

__attribute__((destructor)) static void fini() { printf("f"); }
// The main module stays loaded until process exit, so its destructor is last.

int main() {
  printf("c");
  void* handle = dlopen("libside.so", RTLD_NOW | RTLD_LOCAL);
  assert(handle);

  // `dlopen` completes the shared object's initialization before returning,
  // so the side library's constructor prints `b` before we continue in `main`.
  printf("c");

  int result = dlclose(handle);
  assert(result == 0);

  // `dlclose` runs the shared object's destructor before returning, so `d`
  // appears before this final print from `main`.
  printf("e");

  return 0;
}
