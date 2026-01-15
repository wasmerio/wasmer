#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <pthread.h>

_Thread_local int toast = 10;

void print_toast() {
    printf("value=%d ", toast++);
}

void *thread_func(void *data) {
    print_toast();
    return NULL;
}

int main()
{
    print_toast();
    print_toast();

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

    print_toast();
    return 0;
}