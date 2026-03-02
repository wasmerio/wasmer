#include <pthread.h>
#include <cstdio>
#include <cassert>

int main() {
    pthread_key_t key;
    int res = pthread_key_create(&key, nullptr);
    assert(res == 0);

    void* value = (void*)0x12345;
    res = pthread_setspecific(key, value);
    assert(res == 0);

    void* retrieved = pthread_getspecific(key);
    printf("thread_getspecific returned: %p\n", retrieved);

    assert(retrieved == value);

    pthread_key_delete(key);
    return 0;
}
