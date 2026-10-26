// A raw-mode TUI stand-in with CPU-bound child jobs, exercised through Foot
// and the VM's emulated keyboard. Never run this load against the host session.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>

int main(int argc, char **argv) {
    (void)argc;
    if (strstr(argv[0], "build-worker")) {
        volatile unsigned long value = 1;
        for (;;) value = value * 1664525 + 1013904223;
    }
    struct termios raw;
    assert(tcgetattr(STDIN_FILENO, &raw) == 0);
    cfmakeraw(&raw);
    assert(tcsetattr(STDIN_FILENO, TCSANOW, &raw) == 0);
    char worker[PATH_MAX];
    ssize_t length = readlink("/proc/self/exe", worker, sizeof(worker) - 1);
    assert(length > 0);
    worker[length] = 0;
    char *name = strrchr(worker, '/');
    assert(name && (size_t)(name - worker) + 14 < sizeof(worker));
    strcpy(name + 1, "build-worker");
    for (int i = 0; i < 8; ++i) {
        pid_t pid = fork();
        assert(pid >= 0);
        if (!pid) {
            execl(worker, worker, NULL);
            _exit(127);
        }
    }
    int output = open("/tmp/sekka-keyboard-input", O_WRONLY | O_CREAT | O_TRUNC, 0600);
    assert(output >= 0);
    puts("Ready for keyboard input while eight child jobs run.");
    fflush(stdout);
    for (;;) {
        char input[256];
        ssize_t count = read(STDIN_FILENO, input, sizeof(input));
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) break;
        assert(write(output, input, count) == count);
    }
    return 0;
}
