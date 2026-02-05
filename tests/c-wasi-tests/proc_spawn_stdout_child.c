#include <unistd.h>

int main(void)
{
    const char msg[] = "hello\n";
    ssize_t wrote = write(1, msg, sizeof(msg) - 1);
    return wrote == (ssize_t)(sizeof(msg) - 1) ? 0 : 1;
}
