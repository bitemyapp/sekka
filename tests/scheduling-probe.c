// Scheduling fixture: existing/late threads, fork without exec, and argv[0]
// rewritten as Chromium does. Only run inside the disposable tuning VM.
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <unistd.h>

static void *worker(void *unused) {
    (void)unused;
    for (;;) pause();
    return NULL;
}

static const char *job;

int main(int argc, char **argv) {
    if (argc > 2 && strcmp(argv[1], "jobs") == 0) job = argv[2];
    sigset_t signals;
    sigemptyset(&signals);
    sigaddset(&signals, SIGUSR1);
    sigaddset(&signals, SIGUSR2);
    assert(pthread_sigmask(SIG_BLOCK, &signals, NULL) == 0);
    if (argc > 1 && strcmp(argv[1], "retitle") == 0) {
        const char *title = "chrome --type=renderer";
        assert(strlen(argv[0]) >= strlen(title));
        memset(argv[0], 0, strlen(argv[0]));
        strcpy(argv[0], title);
        prctl(PR_SET_NAME, "CrRendererMain", 0, 0, 0);
    }
    for (int i = 0; i < 4; ++i) {
        pthread_t thread;
        assert(pthread_create(&thread, NULL, worker, NULL) == 0);
        pthread_detach(thread);
    }
    for (;;) {
        int signal;
        assert(sigwait(&signals, &signal) == 0);
        if (signal == SIGUSR1) {
            // A fork-only child of a browser whose own priority changed.
            // The event must classify the child, not renice its parent.
            if (!job && getpriority(PRIO_PROCESS, 0) < 0)
                assert(setpriority(PRIO_PROCESS, 0, 0) == 0);
            pid_t child = fork();
            assert(child >= 0);
            if (!child) {
                if (job) {
                    execl(job, job, NULL);
                    _exit(127);
                }
                for (;;) pause();
            }
        } else {
            // pthreads can explicitly request normal policy even when the
            // creating thread is idle. The fork event must cover that too.
            pthread_attr_t attr;
            pthread_t thread;
            assert(pthread_attr_init(&attr) == 0);
            assert(pthread_attr_setinheritsched(&attr, PTHREAD_EXPLICIT_SCHED) == 0);
            assert(pthread_attr_setschedpolicy(&attr, SCHED_OTHER) == 0);
            assert(pthread_create(&thread, &attr, worker, NULL) == 0);
            pthread_attr_destroy(&attr);
            pthread_detach(thread);
        }
    }
}
