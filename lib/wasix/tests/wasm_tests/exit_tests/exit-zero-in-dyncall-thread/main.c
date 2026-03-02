#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <unistd.h>
#include <pthread.h>
#if __has_include(<wasix/call_dynamic.h>)
#include <wasix/call_dynamic.h>

void dynamically_called() {
    printf("Dyncall in thread\n");
    exit(0);
}

void *thread_func(void *data) {
    wasix_call_dynamic((wasix_function_pointer_t)dynamically_called, NULL, 0, NULL, 0, true);
}
#else
// Mock so the tests compiles on other platforms
// TODO: Remove once our test runner can have platform-specific tests
void *thread_func(void *data) {
    printf("Dyncall in thread\n");
    exit(0);
}
#endif

int main()
{
    pthread_attr_t attr = {0};
    if (pthread_attr_init(&attr) != 0) {
        perror("init attr");
        return -1;
    }

    pthread_t thread = {0};
    if (pthread_create(&thread, &attr, &thread_func, (void *)stdout) != 0) {
        perror("create thread");
        return -1;
    }

    void *thread_ret;
    if (pthread_join(thread, &thread_ret) != 0) {
        perror("join");
        return -1;
    }
    sleep(1);

    return 1;
}