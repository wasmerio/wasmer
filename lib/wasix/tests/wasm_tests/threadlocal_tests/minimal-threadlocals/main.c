#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <pthread.h>

extern _Thread_local int toast;
void *thread_func(void *data) {
    printf("%d\n", toast);
    return NULL;
}

int main() {
    toast = 10;
    printf("%d ", toast);

    pthread_attr_t attr = {0};
    pthread_t thread = {0};
    void *thread_ret;
    if (pthread_attr_init(&attr) != 0) { return 1; }
    if (pthread_create(&thread, &attr, &thread_func, (void *)stdout) != 0) { return 2; }
    if (pthread_join(thread, &thread_ret) != 0) { return 3; }
    return 0;
}