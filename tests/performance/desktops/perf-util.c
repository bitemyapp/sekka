// Small helpers for the desktop comparison's harness, inside each VM.
//
//   perf-util clock        print CLOCK_MONOTONIC in nanoseconds
//   perf-util spin N       N threads that only compute, until killed
//                          (the "under load" condition)
#define _GNU_SOURCE
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static void *spin(void *arg) {
    volatile uint64_t x = (uintptr_t)arg | 1;
    for (;;) x = x * 6364136223846793005ull + 1442695040888963407ull;
    return NULL;
}

int main(int argc, char **argv) {
    if (argc >= 2 && !strcmp(argv[1], "clock")) {
        struct timespec ts;
        clock_gettime(CLOCK_MONOTONIC, &ts);
        printf("%llu\n", (unsigned long long)ts.tv_sec * 1000000000ull + (unsigned long long)ts.tv_nsec);
        return 0;
    }
    if (argc >= 3 && !strcmp(argv[1], "spin")) {
        int n = atoi(argv[2]);
        for (int i = 1; i < n; i++) {
            pthread_t t;
            pthread_create(&t, NULL, spin, (void *)(uintptr_t)i);
        }
        spin(NULL);
    }
    fprintf(stderr, "usage: %s clock | spin N\n", argv[0]);
    return 2;
}
