#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdatomic.h>

static pthread_t thread_self_id;

static void *thread_record_self(void *arg)
{
    (void)arg;
    thread_self_id = pthread_self();
    return NULL;
}

static void *thread_return_value(void *arg)
{
    int *out = (int *)arg;
    *out = 42;
    return (void *)0x1234;
}

static void *thread_arg_int(void *arg)
{
    intptr_t v = (intptr_t)arg;
    int *out = (int *)malloc(sizeof(int));
    assert(out != NULL);
    *out = (int)v;
    return out;
}

static void *thread_arg_array(void *arg)
{
    int *arr = (int *)arg;
    int *out = (int *)malloc(sizeof(int));
    assert(out != NULL);
    int sum = 0;
    for (int i = 0; i < 5; ++i) {
        sum += arr[i];
    }
    *out = sum;
    return out;
}

static atomic_int multi_counter;
static pthread_t multi_ids[4];

static void *thread_multi(void *arg)
{
    intptr_t idx = (intptr_t)arg;
    multi_ids[idx] = pthread_self();
    atomic_fetch_add(&multi_counter, 1);
    return NULL;
}

static void test_basic_create_join(void)
{
    printf("Test 1: basic pthread_create + pthread_join returns value\n");
    pthread_t th;
    int out = 0;
    int rc = pthread_create(&th, NULL, thread_return_value, &out);
    assert(rc == 0);

    void *ret = NULL;
    rc = pthread_join(th, &ret);
    assert(rc == 0);
    assert(out == 42);
    assert(ret == (void *)0x1234);
}

static void test_thread_id_differs(void)
{
    printf("Test 2: new thread id differs from main\n");
    pthread_t th;
    int rc = pthread_create(&th, NULL, thread_record_self, NULL);
    assert(rc == 0);

    rc = pthread_join(th, NULL);
    assert(rc == 0);

    pthread_t main_th = pthread_self();
    assert(pthread_equal(th, main_th) == 0);
}

static void test_thread_id_matches_self(void)
{
    printf("Test 3: pthread_create returns id matching pthread_self in thread\n");
    pthread_t th;
    int rc = pthread_create(&th, NULL, thread_record_self, NULL);
    assert(rc == 0);

    rc = pthread_join(th, NULL);
    assert(rc == 0);

    assert(pthread_equal(th, thread_self_id) != 0);
}

static void test_argument_passing(void)
{
    printf("Test 4: argument passing (int + array)\n");
    pthread_t th1;
    int rc = pthread_create(&th1, NULL, thread_arg_int, (void *)(intptr_t)7);
    assert(rc == 0);

    void *ret1 = NULL;
    rc = pthread_join(th1, &ret1);
    assert(rc == 0);
    assert(ret1 != NULL);
    assert(*(int *)ret1 == 7);
    free(ret1);

    int arr[5] = {1, 2, 3, 4, 5};
    pthread_t th2;
    rc = pthread_create(&th2, NULL, thread_arg_array, arr);
    assert(rc == 0);

    void *ret2 = NULL;
    rc = pthread_join(th2, &ret2);
    assert(rc == 0);
    assert(ret2 != NULL);
    assert(*(int *)ret2 == 15);
    free(ret2);
}

static void test_multiple_threads(void)
{
    printf("Test 5: multiple threads execute\n");
    atomic_store(&multi_counter, 0);
    memset(multi_ids, 0, sizeof(multi_ids));

    pthread_t threads[4];
    for (int i = 0; i < 4; ++i) {
        int rc = pthread_create(&threads[i], NULL, thread_multi, (void *)(intptr_t)i);
        assert(rc == 0);
    }

    for (int i = 0; i < 4; ++i) {
        int rc = pthread_join(threads[i], NULL);
        assert(rc == 0);
    }

    assert(atomic_load(&multi_counter) == 4);

    for (int i = 0; i < 4; ++i) {
        for (int j = i + 1; j < 4; ++j) {
            assert(pthread_equal(multi_ids[i], multi_ids[j]) == 0);
        }
    }
}

int main(void)
{
    test_basic_create_join();
    test_thread_id_differs();
    test_thread_id_matches_self();
    test_argument_passing();
    test_multiple_threads();
    printf("All tests passed!\n");
    return 0;
}
