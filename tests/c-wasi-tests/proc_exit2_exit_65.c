#include <assert.h>
#include <stdlib.h>

int main(void)
{
    const int code = 65;
    assert(code == 65);
    exit(code);
    assert(0 && "exit returned");
    return 0;
}
