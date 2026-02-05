#include <assert.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv)
{
    assert(argc >= 2);
    assert(strcmp(argv[1], "canary") == 0);

    const char *v = getenv("LTP_TEST_ENV_VAR");
    assert(v != NULL);
    assert(strcmp(v, "test") == 0);
    return 0;
}
