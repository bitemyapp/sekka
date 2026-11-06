// Preload shim for the Tatami app reproductions (TAP-03 … TAP-25).
// It refuses Unix-socket connections to host services (/run, /var/run, and
// every X display but the private Xvfb named by SEKKA_REPRO_X_SOCKET), all
// TCP/IP connections, and signals to any process that is not the process
// itself or a descendant of SEKKA_REPRO_ROOT_PID. Refusals are logged to
// SEKKA_REPRO_SHIM_LOG. Tests check `sekka_repro_shim_loaded` before relying
// on it.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <signal.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <unistd.h>

int sekka_repro_shim_loaded = 1;

static void note(const char *fmt, ...) {
  const char *log = getenv("SEKKA_REPRO_SHIM_LOG");
  if (!log) return;
  FILE *f = fopen(log, "a");
  if (!f) return;
  va_list ap;
  va_start(ap, fmt);
  fprintf(f, "[%d] ", (int)getpid());
  vfprintf(f, fmt, ap);
  fputc('\n', f);
  va_end(ap);
  fclose(f);
}

static int allowed_unix(const char *p) {
  if (strncmp(p, "/run/", 5) == 0 || strncmp(p, "/var/run/", 9) == 0) return 0;
  if (strncmp(p, "/tmp/.X11-unix/", 15) == 0) {
    const char *mine = getenv("SEKKA_REPRO_X_SOCKET");
    return mine && strcmp(p, mine) == 0;
  }
  /* The isolation scratch directory (reproductions/isolate exports it). */
  const char *scratch = getenv("ISOLATE_SCRATCH");
  if (scratch && *scratch && strncmp(p, scratch, strlen(scratch)) == 0) return 1;
  /* Private test buses (unix:tmpdir=/tmp); the session bus is under /run/user. */
  if (strncmp(p, "/tmp/dbus-", 10) == 0) return 1;
  return 0;
}

int connect(int fd, const struct sockaddr *addr, socklen_t len) {
  static int (*real)(int, const struct sockaddr *, socklen_t);
  if (!real) real = (int (*)(int, const struct sockaddr *, socklen_t))dlsym(RTLD_NEXT, "connect");
  if (addr && addr->sa_family == AF_UNIX && len > offsetof(struct sockaddr_un, sun_path)) {
    const struct sockaddr_un *un = (const struct sockaddr_un *)addr;
    size_t n = len - offsetof(struct sockaddr_un, sun_path);
    if (n > sizeof un->sun_path) n = sizeof un->sun_path;
    int abstract = un->sun_path[0] == '\0';
    char path[sizeof un->sun_path + 1];
    size_t start = abstract ? 1 : 0;
    size_t m = n > start ? n - start : 0;
    memcpy(path, un->sun_path + start, m);
    path[m] = '\0';
    if (!allowed_unix(path)) {
      note("connect refused: %s%s", abstract ? "@" : "", path);
      errno = ECONNREFUSED;
      return -1;
    }
  } else if (addr && (addr->sa_family == AF_INET || addr->sa_family == AF_INET6)) {
    note("connect refused: an IP address");
    errno = ENETUNREACH;
    return -1;
  }
  return real(fd, addr, len);
}

static int descendant(pid_t pid) {
  const char *root_text = getenv("SEKKA_REPRO_ROOT_PID");
  if (!root_text) return 0;
  pid_t root = (pid_t)atoi(root_text);
  for (int depth = 0; pid > 1 && depth < 64; depth++) {
    if (pid == root) return 1;
    char path[64];
    snprintf(path, sizeof path, "/proc/%d/stat", (int)pid);
    FILE *f = fopen(path, "r");
    if (!f) return 0;
    char buf[1024];
    size_t n = fread(buf, 1, sizeof buf - 1, f);
    fclose(f);
    buf[n] = '\0';
    char *end = strrchr(buf, ')');
    char state;
    int ppid;
    if (!end || sscanf(end + 2, "%c %d", &state, &ppid) != 2) return 0;
    pid = (pid_t)ppid;
  }
  return 0;
}

int kill(pid_t pid, int sig) {
  static int (*real)(pid_t, int);
  if (!real) real = (int (*)(pid_t, int))dlsym(RTLD_NEXT, "kill");
  if (pid == getpid() || (pid > 0 && descendant(pid))) return real(pid, sig);
  note("kill refused: pid %d signal %d", (int)pid, sig);
  errno = EPERM;
  return -1;
}
