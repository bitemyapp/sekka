// Key presses for the desktop comparison's latency probe (wl-probe.c).
//
//   uinput-keys COUNT MIN_MS MAX_MS OUT.tsv [SETTLE_MS]
//
// Creates a virtual USB keyboard through /dev/uinput (root), waits SETTLE_MS
// (default 2000) for the compositor to add it, then presses and releases A
// COUNT times, a random MIN_MS..MAX_MS apart (so presses don't lock to the
// display's frame phase). Records CLOCK_MONOTONIC just before each press is
// written. Only run inside the disposable benchmark VMs.
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/uinput.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static void sleep_ms(double ms) {
    struct timespec ts = {.tv_sec = (time_t)(ms / 1000), .tv_nsec = (long)((ms - (time_t)(ms / 1000) * 1000) * 1e6)};
    while (nanosleep(&ts, &ts) != 0) {
    }
}

static void emit(int fd, int type, int code, int value) {
    struct input_event ev;
    memset(&ev, 0, sizeof ev);
    ev.type = type;
    ev.code = code;
    ev.value = value;
    if (write(fd, &ev, sizeof ev) != sizeof ev) {
        perror("write");
        exit(1);
    }
}

int main(int argc, char **argv) {
    if (argc < 5) {
        fprintf(stderr, "usage: %s COUNT MIN_MS MAX_MS OUT.tsv [SETTLE_MS]\n", argv[0]);
        return 2;
    }
    int count = atoi(argv[1]);
    double lo = atof(argv[2]), hi = atof(argv[3]);
    int settle = argc > 5 ? atoi(argv[5]) : 2000;
    int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
    if (fd < 0) {
        perror("/dev/uinput");
        return 1;
    }
    ioctl(fd, UI_SET_EVBIT, EV_KEY);
    // A full set of ordinary keys, so udev and libinput take it for a keyboard.
    for (int k = KEY_ESC; k <= KEY_KPDOT; k++) ioctl(fd, UI_SET_KEYBIT, k);
    struct uinput_setup setup;
    memset(&setup, 0, sizeof setup);
    setup.id.bustype = BUS_USB;
    setup.id.vendor = 0x1d6b;
    setup.id.product = 0x0104;
    strcpy(setup.name, "perf-probe-keyboard");
    if (ioctl(fd, UI_DEV_SETUP, &setup) < 0 || ioctl(fd, UI_DEV_CREATE) < 0) {
        perror("uinput setup");
        return 1;
    }
    sleep_ms(settle);
    FILE *out = fopen(argv[4], "w");
    if (!out) {
        perror(argv[4]);
        return 1;
    }
    fprintf(out, "idx\tpress_ns\trelease_ns\n");
    srand((unsigned)now_ns());
    for (int i = 0; i < count; i++) {
        sleep_ms(lo + (hi - lo) * (rand() / (double)RAND_MAX));
        uint64_t t = now_ns();
        emit(fd, EV_KEY, KEY_A, 1);
        emit(fd, EV_SYN, SYN_REPORT, 0);
        sleep_ms(25);
        uint64_t r = now_ns();
        emit(fd, EV_KEY, KEY_A, 0);
        emit(fd, EV_SYN, SYN_REPORT, 0);
        fprintf(out, "%d\t%llu\t%llu\n", i, (unsigned long long)t, (unsigned long long)r);
    }
    fclose(out);
    sleep_ms(500);
    ioctl(fd, UI_DEV_DESTROY);
    close(fd);
    return 0;
}
