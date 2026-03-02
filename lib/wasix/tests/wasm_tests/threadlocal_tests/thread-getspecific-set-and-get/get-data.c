#include <assert.h>
#include "thread-keys.h"
#include <stdio.h>

void get_data() {
    void* valueA = (void*)0x12345;
    void* valueB = (void*)0x67890;
    void* retrievedA = pthread_getspecific(key_a);
    assert(retrievedA == valueA);
    void* retrievedB = pthread_getspecific(key_b);
    assert(retrievedB == valueB);
    fprintf(stdout, "get");
}
