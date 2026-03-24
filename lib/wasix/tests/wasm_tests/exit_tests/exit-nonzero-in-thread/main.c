#include <stdio.h>
#include <stdlib.h>
#include <dlfcn.h>
#include <unistd.h>
#include <pthread.h>

void* print_and_exit(void *data) {
    printf("Thread called!\n");
    exit(99);  // Exit the program with 0
}

int main()
{
    pthread_attr_t attr = {0};
    if (pthread_attr_init(&attr) != 0) {
        perror("init attr");
        return -1;
    }

    pthread_t thread = {0};
    if (pthread_create(&thread, &attr, &print_and_exit, (void *)stdout) != 0) {
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