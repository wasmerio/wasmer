/* Same runtime conditions as Tcl unix/configure's compatibility probes. */
#include <stdlib.h>
#include <string.h>

int main(void) {
  char c0 = 0x40, c1 = (char)0x80, c2 = (char)0x81;
  if (memcmp(&c0, &c2, 1) >= 0 || memcmp(&c1, &c2, 1) >= 0) return 1;
  for (int i = 0; i < 4; ++i) {
    char a[21], b[21];
    strcpy(a + i, "--------01111111");
    strcpy(b + i, "--------10000000");
    if (memcmp(a + i, b + i, 16) >= 0) return 2;
  }
  if (strstr("\0test", "test")) return 3;
  char *end, *string = "0";
  return strtoul(string, &end, 0) != 0 || end != string + 1;
}
