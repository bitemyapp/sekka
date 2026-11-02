/* Sample the AMD socket power sensor (amdgpu hwmon "PPT": CPU cores, iGPU,
 * SoC/IO die) with absolute CLOCK_MONOTONIC timestamps, so the harness can
 * integrate energy over any window it timed with the same clock.
 *
 *   ppt-sampler INTERVAL_MS OUTPUT.tsv
 *
 * Each line: monotonic nanoseconds, then the reading in milliwatts. On this
 * 9800X3D the driver reports whole watts x1000 despite the hwmon ABI's
 * microwatts; the harness records the raw value and its unit assumption.
 * Runs until SIGTERM/SIGINT. Same sensor and trapezoidal integration as
 * tests/performance/powermeter.c (used for the other comparisons), which
 * reports relative times. */
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t stop;
static void on_signal(int s) { (void)s; stop = 1; }

static int find_ppt(char *out, size_t n) {
    DIR *d = opendir("/sys/class/hwmon");
    if (!d) return -1;
    struct dirent *e;
    int found = -1;
    while ((e = readdir(d))) {
        if (e->d_name[0] == '.') continue;
        char p[512], buf[64];
        FILE *f;
        snprintf(p, sizeof p, "/sys/class/hwmon/%s/name", e->d_name);
        if (!(f = fopen(p, "r"))) continue;
        if (!fgets(buf, sizeof buf, f)) { fclose(f); continue; }
        fclose(f);
        if (strncmp(buf, "amdgpu", 6)) continue;
        snprintf(p, sizeof p, "/sys/class/hwmon/%s/power1_label", e->d_name);
        if (!(f = fopen(p, "r"))) continue;
        if (!fgets(buf, sizeof buf, f)) { fclose(f); continue; }
        fclose(f);
        if (strncmp(buf, "PPT", 3)) continue;
        snprintf(out, n, "/sys/class/hwmon/%s/power1_input", e->d_name);
        found = 0;
        break;
    }
    closedir(d);
    return found;
}

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: ppt-sampler INTERVAL_MS OUTPUT.tsv\n"); return 2; }
    int ms = atoi(argv[1]);
    if (ms < 1 || ms > 1000) { fprintf(stderr, "ppt-sampler: interval 1..1000 ms\n"); return 2; }
    char path[512];
    if (find_ppt(path, sizeof path)) { fprintf(stderr, "ppt-sampler: no amdgpu PPT sensor\n"); return 1; }
    int fd = open(path, O_RDONLY);
    if (fd < 0) { perror(path); return 1; }
    FILE *out = fopen(argv[2], "w");
    if (!out) { perror(argv[2]); return 1; }
    fprintf(out, "# sensor %s interval_ms %d unit mW\n", path, ms);
    fflush(out);
    struct sigaction sa = {0};
    sa.sa_handler = on_signal;
    sigaction(SIGINT, &sa, 0);
    sigaction(SIGTERM, &sa, 0);
    struct timespec next;
    clock_gettime(CLOCK_MONOTONIC, &next);
    unsigned long count = 0;
    while (!stop) {
        char buf[32];
        ssize_t r = pread(fd, buf, sizeof buf - 1, 0);
        struct timespec t;
        clock_gettime(CLOCK_MONOTONIC, &t);
        if (r > 0) {
            buf[r] = 0;
            fprintf(out, "%lld\t%ld\n", (long long)t.tv_sec * 1000000000LL + t.tv_nsec, strtol(buf, NULL, 10));
            if (++count % 100 == 0) fflush(out);
        }
        next.tv_nsec += (long)ms * 1000000L;
        while (next.tv_nsec >= 1000000000L) { next.tv_nsec -= 1000000000L; next.tv_sec++; }
        clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &next, NULL);
    }
    fflush(out);
    fclose(out);
    return 0;
}
