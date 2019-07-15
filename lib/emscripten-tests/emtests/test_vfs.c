#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>

int main() {
    char data[256] = {0};
    ssize_t fd = open("data.txt", 0);
    ssize_t result = read((int)fd, &data, 255);
    printf("content: %s", data);
    printf("fd: %zd\n", fd);
    return 0;
}
