#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <pthread.h>

_Thread_local int toast = 10;
extern void print_toast_from_lib();
extern void increment_toast_from_lib();
void increment_toast_from_main() {
    toast++;
}
void print_toast_from_main() {
    printf("%d", toast);
}

void print_main_and_lib() {
    print_toast_from_lib();
    printf(":");
    print_toast_from_main();
    printf(" ");
}

void *thread_func(void *data) {
    print_main_and_lib();
    increment_toast_from_lib();
    print_main_and_lib();
    increment_toast_from_lib();
    print_main_and_lib();
    return NULL;
}

int main()
{
    print_main_and_lib();
    increment_toast_from_lib();
    print_main_and_lib();
    increment_toast_from_lib();
    print_main_and_lib();

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

    return 0;
}