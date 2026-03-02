#include <errno.h>
#include <stdio.h>
#include <thread>
#include <vector>

constexpr int NUM_THREADS = 4;
int thread_values[NUM_THREADS];

void worker(int idx) {
    errno = 100 + idx;
    thread_values[idx] = errno;
}

int main() {
    errno = 1;
    std::vector<std::thread> threads;
    threads.reserve(NUM_THREADS);
    for (int i = 0; i < NUM_THREADS; ++i) {
        threads.emplace_back(worker, i);
    }
    for (auto &t : threads) {
        t.join();
    }
    printf("main errno %d\n", errno);
    for (int i = 0; i < NUM_THREADS; ++i) {
        printf("thread %d errno %d\n", i, thread_values[i]);
    }
    return 0;
}
