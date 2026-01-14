#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <sys/types.h>

// For WASIX, we use a custom syscall to get thread ID
// In WASIX, thread IDs are sequential from 0
#define __WASI_THREAD_ID 34
extern int __wasi_thread_id(int *tid);

static int get_thread_id(void)
{
    int tid;
    if (__wasi_thread_id(&tid) != 0)
    {
        return -1;
    }
    return tid;
}

#define NUM_THREADS 10

typedef struct
{
    int index;
    int tid;
    pid_t pid;
} thread_data_t;

static thread_data_t thread_results[NUM_THREADS];

static void *thread_func(void *arg)
{
    int index = *(int *)arg;
    thread_results[index].index = index;
    thread_results[index].tid = get_thread_id();
    thread_results[index].pid = getpid();

    // Small delay to ensure threads overlap
    usleep(1000);

    return NULL;
}

int main()
{
    // Test 1: Basic thread_id - should return valid TID (>= 0)
    // From LTP gettid01.c pattern - validates TID is in valid range
    printf("Test 1: Basic thread_id validation\n");
    int main_tid = get_thread_id();
    if (main_tid < 0)
    {
        fprintf(stderr, "get_thread_id() returned invalid TID: %d\n", main_tid);
        return 1;
    }
    printf("  Main thread TID: %d (valid)\n", main_tid);

    // Test 2: Consistency - repeated calls should return same value
    // From stress-ng stress-get.c pattern - repeated gettid calls
    printf("Test 2: Consistency across multiple calls\n");
    int tid1 = get_thread_id();
    int tid2 = get_thread_id();
    int tid3 = get_thread_id();

    if (tid1 != tid2 || tid2 != tid3)
    {
        fprintf(stderr, "get_thread_id() inconsistent: %d, %d, %d\n", tid1, tid2, tid3);
        return 1;
    }
    printf("  All calls returned same TID: %d\n", tid1);

    // Test 3: Stress test - 1000 calls should all return same value
    // From stress-ng stress-get.c pattern - high frequency calls
    printf("Test 3: Stress test (1000 calls)\n");
    for (int i = 0; i < 1000; i++)
    {
        int tid = get_thread_id();
        if (tid != main_tid)
        {
            fprintf(stderr, "TID changed on iteration %d: expected %d, got %d\n",
                    i, main_tid, tid);
            return 1;
        }
    }
    printf("  All 1000 calls consistent\n");

    // Test 4: Multi-threaded uniqueness
    // From LTP gettid02.c - validates unique TIDs across threads
    printf("Test 4: Multi-threaded TID uniqueness\n");

    pthread_t threads[NUM_THREADS];
    int thread_args[NUM_THREADS];

    // Create threads
    for (int i = 0; i < NUM_THREADS; i++)
    {
        thread_args[i] = i;
        if (pthread_create(&threads[i], NULL, thread_func, &thread_args[i]) != 0)
        {
            fprintf(stderr, "Failed to create thread %d\n", i);
            return 1;
        }
    }

    // Wait for all threads
    for (int i = 0; i < NUM_THREADS; i++)
    {
        if (pthread_join(threads[i], NULL) != 0)
        {
            fprintf(stderr, "Failed to join thread %d\n", i);
            return 1;
        }
    }

    // Verify all threads got valid TIDs
    printf("  Verifying thread TIDs...\n");
    for (int i = 0; i < NUM_THREADS; i++)
    {
        if (thread_results[i].tid < 0)
        {
            fprintf(stderr, "Thread %d has invalid TID: %d\n", i, thread_results[i].tid);
            return 1;
        }
    }

    // Verify all threads have different TIDs from main thread
    for (int i = 0; i < NUM_THREADS; i++)
    {
        if (thread_results[i].tid == main_tid)
        {
            fprintf(stderr, "Thread %d has same TID as main thread: %d\n",
                    i, thread_results[i].tid);
            return 1;
        }
    }

    // Verify all threads have unique TIDs (no duplicates)
    int found_duplicate = 0;
    for (int i = 0; i < NUM_THREADS; i++)
    {
        for (int j = i + 1; j < NUM_THREADS; j++)
        {
            if (thread_results[i].tid == thread_results[j].tid)
            {
                fprintf(stderr, "Thread %d and thread %d have same TID: %d\n",
                        i, j, thread_results[i].tid);
                found_duplicate = 1;
            }
        }
    }

    if (found_duplicate)
    {
        return 1;
    }

    printf("  All %d threads have unique TIDs\n", NUM_THREADS);

    // Print summary
    printf("  Thread TID summary:\n");
    printf("    Main thread: TID=%d\n", main_tid);
    for (int i = 0; i < NUM_THREADS; i++)
    {
        printf("    Thread %d: TID=%d\n", i, thread_results[i].tid);
    }

    printf("All tests passed!\n");
    return 0;
}
