#include <thread>
#include <iostream>

struct yeah { ~yeah() { std::cout << "destruct thread local\n"; } };
thread_local yeah x;
int main() { std::thread{ []() { std::cout << "hello\n"; } }.join(); }