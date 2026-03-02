#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <semaphore.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <string.h>

// Even though POSIX requires the name to start with a slash, Linux also allows names with no or multiple leading slashes.
// However, names with embedded slashes are not allowed.
#define VALID_NAME_1 "/valid"
#define VALID_NAME_2 "valid"
#define VALID_NAME_3 "//////valid"
// #define VALID_NAME_4 "/." // Valid on POSIX, but not with musl
// #define VALID_NAME_5 "/.."
#define VALID_NAME_6 "/valid.name"
#define VALID_NAME_7 "/valid.<>:'\\|\"?*name"
#define VALID_NAME_8 "/embedded\0null" // Equivalent to "/embedded" ... why am I even testing this?
// #define VALID_NAME_9 "."
// #define VALID_NAME_10 ".."
#define INVALID_NAME_1 ""
#define INVALID_NAME_2 "/embedded/slash"
#define INVALID_NAME_3 "/name-that-is-way-too-long-123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678"

// // NULL pointer is also invalid but results in a segmentation fault instead of sem_open returning SEM_FAILED
// #define INVALID_NAME_99 NULL
void expect_valid_name(const char* name) {
    sem_t* sem = sem_open(name, O_CREAT, 0600, 0);
    if (sem == SEM_FAILED) {
        fprintf(stderr, "sem_open failed with a valid name: %s\n", name);
        perror("sem_open");
        sem_unlink(name); // Don't check for errors, just best-effort cleanup
        exit(EXIT_FAILURE);
    }
    sem_unlink(name);
}

void expect_invalid_name(const char* name) {
    sem_t* sem = sem_open(name, O_CREAT, 0600, 0);
    if (sem != SEM_FAILED) {
        fprintf(stderr, "sem_open worked with an invalid name: %s\n", name);
        sem_unlink(name); // Don't check for errors, just best-effort cleanup
        exit(EXIT_FAILURE);
    }
    sem_unlink(name);
}

int main(void) {
    expect_valid_name(VALID_NAME_1);
    expect_valid_name(VALID_NAME_2);
    expect_valid_name(VALID_NAME_3);
    // expect_valid_name(VALID_NAME_4);
    // expect_valid_name(VALID_NAME_5);
    expect_valid_name(VALID_NAME_6);
    expect_valid_name(VALID_NAME_7);
    expect_valid_name(VALID_NAME_8);
    // expect_valid_name(VALID_NAME_9);
    // expect_valid_name(VALID_NAME_10);
    expect_invalid_name(INVALID_NAME_1);
    expect_invalid_name(INVALID_NAME_2);
    expect_invalid_name(INVALID_NAME_3);
    // expect_invalid_name(INVALID_NAME_99); // This one causes a segmentation fault instead

    puts("done.");
    return EXIT_SUCCESS;
}