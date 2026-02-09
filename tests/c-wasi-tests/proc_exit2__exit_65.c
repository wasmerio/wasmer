#include <assert.h>
#include <stdlib.h>

int main(void)
{
    const int code = 65;
    assert(code == 65);
    _Exit(code);
    assert(0 && "_Exit returned");
    return 0;
}
