#include <string.h>
#include <unistd.h>

int main(void)
{
    const char expected[] = "Hello, posix_spawn";
    char buf[sizeof(expected)];
    ssize_t want = (ssize_t)(sizeof(expected) - 1);

    if (read(10, buf, want) != want) {
        return 1;
    }
    buf[want] = '\0';
    if (close(10) != 0) {
        return 2;
    }
    if (strcmp(buf, expected) != 0) {
        return 3;
    }
    return 0;
}
