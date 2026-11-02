// powermeter: sample the AMD socket's power while a command runs (or until
// SIGINT/SIGTERM) and report the energy.
//
//   powermeter [-i MS] [-o SAMPLES.tsv] [-j SUMMARY.json] [-- CMD ARGS...]
//
// The sensor is the amdgpu hwmon input labelled "PPT" (power1_input): the
// socket's power as its SMU reports it (CPU cores, integrated GPU, I/O die),
// which any user can read on Ryzen processors with integrated graphics. On
// the 9800X3D it was measured on, the value is in milliwatts at whole-watt
// resolution, updated about every 7 ms, although hwmon's ABI says
// microwatts: about 18 W idle, 69 W with four threads spinning, 126 W with
// eight, within 1.5% of RAPL's package counter over the same window.
//
// Energy is the trapezoidal integral of the samples over CLOCK_MONOTONIC.
// The summary also gives the system's busy CPU seconds (/proc/stat) over
// the window. Build: cc -O2 -o powermeter powermeter.c
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <fcntl.h>

static volatile sig_atomic_t stop = 0;
static void on_sig(int s) { (void)s; stop = 1; }

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static int find_ppt(char *out, size_t n) {
  DIR *d = opendir("/sys/class/hwmon"); if (!d) return -1;
  struct dirent *e; int ok = -1;
  while ((e = readdir(d))) {
    if (e->d_name[0] == '.') continue;
    char p[512], buf[64]; FILE *f;
    snprintf(p, sizeof p, "/sys/class/hwmon/%s/name", e->d_name);
    if (!(f = fopen(p, "r"))) continue;
    if (!fgets(buf, sizeof buf, f)) { fclose(f); continue; } fclose(f);
    if (strncmp(buf, "amdgpu", 6)) continue;
    snprintf(p, sizeof p, "/sys/class/hwmon/%s/power1_label", e->d_name);
    if (!(f = fopen(p, "r"))) continue;
    if (!fgets(buf, sizeof buf, f)) { fclose(f); continue; } fclose(f);
    if (strncmp(buf, "PPT", 3)) continue;
    snprintf(out, n, "/sys/class/hwmon/%s/power1_input", e->d_name); ok = 0; break;
  }
  closedir(d); return ok;
}

static long read_uw(int fd) {
  char buf[32]; ssize_t r = pread(fd, buf, sizeof buf - 1, 0);
  if (r <= 0)
    return -1;
  buf[r] = 0;
  return strtol(buf, NULL, 10);
}

static double busy_seconds(void) {
  FILE *f = fopen("/proc/stat", "r"); if (!f) return -1;
  unsigned long long u, ni, s, idle, io, irq, sirq, st;
  int r = fscanf(f, "cpu %llu %llu %llu %llu %llu %llu %llu %llu", &u, &ni, &s, &idle, &io, &irq, &sirq, &st);
  fclose(f); if (r < 8) return -1;
  return (double)(u + ni + s + irq + sirq + st) / sysconf(_SC_CLK_TCK);
}

static int cmp(const void *a, const void *b) { double x = *(double*)a, y = *(double*)b; return x < y ? -1 : x > y; }

int main(int argc, char **argv) {
  int ms = 10; const char *samples = NULL, *summary = NULL; int opt;
  while ((opt = getopt(argc, argv, "+i:o:j:")) != -1) {
    if (opt == 'i') ms = atoi(optarg);
    else if (opt == 'o') samples = optarg;
    else if (opt == 'j') summary = optarg;
    else { fprintf(stderr, "usage: powermeter [-i MS] [-o SAMPLES] [-j SUMMARY] [-- CMD...]\n"); return 2; }
  }
  char path[512]; if (find_ppt(path, sizeof path)) { fprintf(stderr, "powermeter: no amdgpu PPT sensor\n"); return 1; }
  int fd = open(path, O_RDONLY); if (fd < 0) { perror(path); return 1; }
  struct sigaction sa = {0}; sa.sa_handler = on_sig; sigaction(SIGINT, &sa, 0); sigaction(SIGTERM, &sa, 0);
  pid_t child = 0;
  FILE *so = samples ? fopen(samples, "w") : NULL;
  size_t cap = 1 << 16, n = 0; double *w = malloc(cap * sizeof *w);
  double t0 = now(), b0 = busy_seconds(), tprev = t0, wprev = read_uw(fd) / 1e3, energy = 0;
  if (optind < argc) {
    child = fork();
    if (child == 0) { execvp(argv[optind], argv + optind); perror("exec"); _exit(127); }
  }
  int status = 0;
  struct timespec iv = { ms / 1000, (ms % 1000) * 1000000L };
  while (!stop) {
    nanosleep(&iv, NULL);
    double t = now(); long uw = read_uw(fd); if (uw < 0) continue;
    double watts = uw / 1e3; // the driver reports whole watts x1000 (mW), despite hwmon ABI saying uW
    energy += (t - tprev) * (watts + wprev) / 2; tprev = t; wprev = watts;
    if (n == cap) { cap *= 2; w = realloc(w, cap * sizeof *w); }
    w[n++] = watts;
    if (so) fprintf(so, "%.6f\t%.3f\n", t - t0, watts);
    if (child > 0) { pid_t r = waitpid(child, &status, WNOHANG); if (r == child) break; }
  }
  if (child > 0 && stop) { kill(child, SIGTERM); waitpid(child, &status, 0); }
  double t1 = now(), b1 = busy_seconds(), dur = t1 - t0;
  qsort(w, n, sizeof *w, cmp);
  double mean = dur > 0 ? energy / dur : 0;
  char js[1024];
  snprintf(js, sizeof js,
    "{\"sensor\":\"%s\",\"interval_ms\":%d,\"duration_s\":%.3f,\"samples\":%zu,\"energy_j\":%.3f,\"mean_w\":%.3f,"
    "\"p10_w\":%.1f,\"p50_w\":%.1f,\"p90_w\":%.1f,\"max_w\":%.1f,\"system_busy_cpu_s\":%.2f,\"exit\":%d}\n",
    path, ms, dur, n, energy, mean, n ? w[n/10] : 0, n ? w[n/2] : 0, n ? w[n*9/10] : 0, n ? w[n-1] : 0,
    b1 - b0, child > 0 ? (WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status)) : 0);
  fputs(js, stderr);
  if (summary) { FILE *f = fopen(summary, "w"); if (f) { fputs(js, f); fclose(f); } }
  if (so) fclose(so);
  return child > 0 && WIFEXITED(status) ? WEXITSTATUS(status) : 0;
}
