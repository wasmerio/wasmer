/* libc2: autoconf AC_FUNC_FORK cannot see libc2's macro-only vfork and emits
 * `#define vfork fork` (config.h); <unistd.h> then declares fork() unavailable.
 * Expected: compiles. */
#define vfork fork
#include <unistd.h>
int main(void){ return fork(); }
