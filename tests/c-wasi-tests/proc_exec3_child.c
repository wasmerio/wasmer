#include <assert.h>
#include <string.h>

int main(int argc, char **argv)
{
    assert(argc >= 2);
    assert(strcmp(argv[1], "canary") == 0);
    return 0;
}
