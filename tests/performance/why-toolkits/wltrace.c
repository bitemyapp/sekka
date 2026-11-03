/* wltrace: a relaying Wayland proxy that time-stamps what a client and its
 * compositor say to each other, for docs/performance/why-toolkits.md.
 *
 *   WAYLAND_DISPLAY=upstream wltrace LISTEN_NAME LOGFILE
 *
 * It listens on $XDG_RUNTIME_DIR/LISTEN_NAME; a client started with
 * WAYLAND_DISPLAY=LISTEN_NAME is connected through it to the compositor at
 * $WAYLAND_DISPLAY. Bytes and file descriptors are passed on unchanged, in
 * order, as they arrive (so it works for any client, whatever Wayland library
 * it uses, the kit's pure-Rust one included); a copy of each direction's
 * stream is parsed to log, with CLOCK_MONOTONIC nanoseconds as each chunk
 * arrived:
 *
 *   client → compositor: wl_surface attach / damage / damage_buffer / frame /
 *     commit, the buffers' kinds and sizes (wl_shm, linux-dmabuf, single
 *     pixel), subsurfaces, xdg_surface ack_configure and window geometry,
 *     wp_presentation feedback requests, globals bound;
 *   compositor → client: wl_keyboard key, wl_pointer motion / button / axis /
 *     frame, wl_callback done for frame callbacks, wp_presentation_feedback
 *     presented / discarded, xdg_toplevel and xdg_surface configure, buffer
 *     releases, protocol errors.
 *
 * Nothing is added to or taken from either stream: the proxy only observes.
 * The one cost is a hop: each chunk is read and written once more.
 *
 * Log lines: "<ns> <client> <event> <fields...>". */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

enum {
  MAX_CLIENTS = 64,
  MAX_FDS = 28,       /* what libwayland and wayland-rs send per sendmsg */
  MAX_FDS_IN = 64,
  CLIENT_IDS = 1 << 20,
  SERVER_IDS = 1 << 16,
};
#define SERVER_ID_START 0xff000000u

enum type {
  T_NONE, T_DISPLAY, T_REGISTRY, T_COMPOSITOR, T_SUBCOMPOSITOR, T_SURFACE, T_FRAME_CB, T_SEAT, T_KEYBOARD,
  T_POINTER, T_SHM, T_POOL, T_BUFFER, T_DMABUF, T_PARAMS, T_PRESENTATION, T_FEEDBACK, T_WM_BASE, T_XDG_SURFACE,
  T_TOPLEVEL, T_SINGLE_PIXEL,
};

struct object {
  uint8_t type;
  char kind;              /* buffers: 's' shm, 'd' dmabuf, 'p' single pixel */
  uint32_t ref;           /* frame callback / feedback / xdg_surface: its surface; toplevel: its xdg_surface */
  int32_t w, h;           /* buffers; dmabuf params: the size asked for */
};

struct stream {
  uint8_t *buf;           /* parse buffer: bytes not yet a whole message */
  size_t len, cap;
  uint8_t *out;           /* bytes waiting to be written to the other side */
  size_t out_len, out_cap;
  int fds[256];
  int nfds;
};

struct client {
  int cfd, sfd;           /* the client's socket, the compositor's */
  int n;
  struct stream up, down; /* client → compositor, compositor → client */
  struct object *ids, *server_ids;
};

static FILE *logf_;
static volatile sig_atomic_t stop;
static struct client *clients[MAX_CLIENTS];
static int next_client = 1;

static uint64_t now_ns(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * 1000000000u + (uint64_t)t.tv_nsec;
}

static struct object *obj(struct client *c, uint32_t id) {
  if (id >= SERVER_ID_START) {
    uint32_t i = id - SERVER_ID_START;
    return i < SERVER_IDS ? &c->server_ids[i] : NULL;
  }
  return id < CLIENT_IDS ? &c->ids[id] : NULL;
}

static void set(struct client *c, uint32_t id, enum type t, uint32_t ref) {
  struct object *o = obj(c, id);
  if (o) { memset(o, 0, sizeof *o); o->type = (uint8_t)t; o->ref = ref; }
}

static uint8_t type_of(struct client *c, uint32_t id) {
  struct object *o = obj(c, id);
  return o ? o->type : T_NONE;
}

/* A string argument at p (length-prefixed, padded); returns the next arg. */
static const uint32_t *str(const uint32_t *p, const uint32_t *end, const char **s) {
  if (p >= end) { *s = ""; return end; }
  uint32_t n = *p++;
  *s = n ? (const char *)p : "";
  p += (n + 3) / 4;
  return p > end ? end : p;
}

static enum type global_type(const char *iface) {
  static const struct { const char *name; enum type t; } known[] = {
    {"wl_compositor", T_COMPOSITOR}, {"wl_subcompositor", T_SUBCOMPOSITOR}, {"wl_seat", T_SEAT},
    {"wl_shm", T_SHM}, {"zwp_linux_dmabuf_v1", T_DMABUF}, {"wp_presentation", T_PRESENTATION},
    {"xdg_wm_base", T_WM_BASE}, {"wp_single_pixel_buffer_manager_v1", T_SINGLE_PIXEL},
  };
  for (size_t i = 0; i < sizeof known / sizeof *known; i++)
    if (!strcmp(iface, known[i].name)) return known[i].t;
  return T_NONE;
}

/* A whole message from the client to the compositor. */
static void request(struct client *c, uint64_t t, uint32_t id, uint32_t op, const uint32_t *a, const uint32_t *end) {
  size_t n = (size_t)(end - a);
  uint8_t ty = type_of(c, id);
  FILE *f = logf_;
  switch (ty) {
  case T_DISPLAY:
    if (op == 1 && n >= 1) set(c, a[0], T_REGISTRY, 0);
    break;
  case T_REGISTRY:
    if (op == 0 && n >= 2) {
      const char *iface;
      const uint32_t *p = str(a + 1, end, &iface);
      if (p + 2 <= end) {
        set(c, p[1], global_type(iface), 0);
        fprintf(f, "%" PRIu64 " %d bind %s %u %u\n", t, c->n, iface, p[0], p[1]);
      }
    }
    break;
  case T_COMPOSITOR:
    if (op == 0 && n >= 1) { set(c, a[0], T_SURFACE, 0); fprintf(f, "%" PRIu64 " %d surface %u\n", t, c->n, a[0]); }
    break;
  case T_SUBCOMPOSITOR:
    if (op == 1 && n >= 3) fprintf(f, "%" PRIu64 " %d subsurface %u %u\n", t, c->n, a[1], a[2]);
    break;
  case T_SURFACE:
    switch (op) {
    case 1: {
      if (n < 3) break;
      struct object *b = a[0] ? obj(c, a[0]) : NULL;
      if (b && b->type == T_BUFFER)
        fprintf(f, "%" PRIu64 " %d attach %u %u %c %d %d\n", t, c->n, id, a[0], b->kind, b->w, b->h);
      else
        fprintf(f, "%" PRIu64 " %d attach %u %u %c 0 0\n", t, c->n, id, a[0], a[0] ? '?' : '-');
      break;
    }
    case 2: case 9:
      if (n >= 4) fprintf(f, "%" PRIu64 " %d %s %u %d %d %d %d\n", t, c->n, op == 2 ? "damage" : "dbuf", id,
                          (int32_t)a[0], (int32_t)a[1], (int32_t)a[2], (int32_t)a[3]);
      break;
    case 3:
      if (n >= 1) { set(c, a[0], T_FRAME_CB, id); fprintf(f, "%" PRIu64 " %d frame %u %u\n", t, c->n, id, a[0]); }
      break;
    case 6:
      fprintf(f, "%" PRIu64 " %d commit %u\n", t, c->n, id);
      break;
    case 8:
      if (n >= 1) fprintf(f, "%" PRIu64 " %d scale %u %d\n", t, c->n, id, (int32_t)a[0]);
      break;
    }
    break;
  case T_SEAT:
    if (op == 0 && n >= 1) set(c, a[0], T_POINTER, 0);
    if (op == 1 && n >= 1) set(c, a[0], T_KEYBOARD, 0);
    break;
  case T_SHM:
    if (op == 0 && n >= 1) set(c, a[0], T_POOL, 0);
    break;
  case T_POOL:
    if (op == 0 && n >= 6) {
      set(c, a[0], T_BUFFER, 0);
      struct object *b = obj(c, a[0]);
      if (b) { b->kind = 's'; b->w = (int32_t)a[2]; b->h = (int32_t)a[3]; }
    }
    break;
  case T_DMABUF:
    if (op == 1 && n >= 1) set(c, a[0], T_PARAMS, 0);
    break;
  case T_PARAMS:
    if (op == 2 && n >= 2) { struct object *p = obj(c, id); if (p) { p->w = (int32_t)a[0]; p->h = (int32_t)a[1]; } }
    if (op == 3 && n >= 3) {
      set(c, a[0], T_BUFFER, 0);
      struct object *b = obj(c, a[0]);
      if (b) { b->kind = 'd'; b->w = (int32_t)a[1]; b->h = (int32_t)a[2]; }
    }
    break;
  case T_SINGLE_PIXEL:
    if (op == 1 && n >= 1) {
      set(c, a[0], T_BUFFER, 0);
      struct object *b = obj(c, a[0]);
      if (b) { b->kind = 'p'; b->w = 1; b->h = 1; }
    }
    break;
  case T_PRESENTATION:
    if (op == 1 && n >= 2) { set(c, a[1], T_FEEDBACK, a[0]); fprintf(f, "%" PRIu64 " %d fb %u %u\n", t, c->n, a[0], a[1]); }
    break;
  case T_WM_BASE:
    if (op == 2 && n >= 2) set(c, a[0], T_XDG_SURFACE, a[1]);
    break;
  case T_XDG_SURFACE:
    if (op == 1 && n >= 1) set(c, a[0], T_TOPLEVEL, id);
    if (op == 3 && n >= 4) fprintf(f, "%" PRIu64 " %d geometry %u %d %d %d %d\n", t, c->n, type_of(c, id) ? obj(c, id)->ref : 0,
                                   (int32_t)a[0], (int32_t)a[1], (int32_t)a[2], (int32_t)a[3]);
    if (op == 4 && n >= 1) fprintf(f, "%" PRIu64 " %d ack %u %u\n", t, c->n, obj(c, id)->ref, a[0]);
    break;
  }
}

/* A whole message from the compositor to the client. */
static void event(struct client *c, uint64_t t, uint32_t id, uint32_t op, const uint32_t *a, const uint32_t *end) {
  size_t n = (size_t)(end - a);
  struct object *o = obj(c, id);
  uint8_t ty = o ? o->type : T_NONE;
  FILE *f = logf_;
  switch (ty) {
  case T_DISPLAY:
    if (op == 0 && n >= 2) {
      const char *msg;
      str(a + 2, end, &msg);
      fprintf(f, "%" PRIu64 " %d error %u %u %s\n", t, c->n, a[0], a[1], msg);
    } else if (op == 1 && n >= 1) {
      struct object *d = obj(c, a[0]);
      if (d) memset(d, 0, sizeof *d);
    }
    break;
  case T_FRAME_CB:
    if (op == 0 && n >= 1) fprintf(f, "%" PRIu64 " %d done %u %u %u\n", t, c->n, o->ref, id, a[0]);
    break;
  case T_KEYBOARD:
    if (op == 3 && n >= 4) fprintf(f, "%" PRIu64 " %d key %u %u %u\n", t, c->n, a[1], a[2], a[3]);
    if (op == 1) fprintf(f, "%" PRIu64 " %d kfocus 1\n", t, c->n);
    if (op == 2) fprintf(f, "%" PRIu64 " %d kfocus 0\n", t, c->n);
    break;
  case T_POINTER:
    if (op == 2 && n >= 3) fprintf(f, "%" PRIu64 " %d motion %u %.2f %.2f\n", t, c->n, a[0], (int32_t)a[1] / 256.0, (int32_t)a[2] / 256.0);
    if (op == 3 && n >= 4) fprintf(f, "%" PRIu64 " %d button %u %u %u\n", t, c->n, a[1], a[2], a[3]);
    if (op == 4 && n >= 3) fprintf(f, "%" PRIu64 " %d axis %u %u %.2f\n", t, c->n, a[0], a[1], (int32_t)a[2] / 256.0);
    if (op == 5) fprintf(f, "%" PRIu64 " %d pframe\n", t, c->n);
    if (op == 0 && n >= 4) fprintf(f, "%" PRIu64 " %d enter %.2f %.2f\n", t, c->n, (int32_t)a[2] / 256.0, (int32_t)a[3] / 256.0);
    break;
  case T_BUFFER:
    if (op == 0) fprintf(f, "%" PRIu64 " %d release %u\n", t, c->n, id);
    break;
  case T_PARAMS:
    if (op == 0 && n >= 1) {
      int32_t w = o->w, h = o->h;
      set(c, a[0], T_BUFFER, 0);
      struct object *b = obj(c, a[0]);
      if (b) { b->kind = 'd'; b->w = w; b->h = h; }
    }
    break;
  case T_FEEDBACK:
    if (op == 1 && n >= 7)
      fprintf(f, "%" PRIu64 " %d presented %u %u %" PRIu64 " %u %u\n", t, c->n, o->ref, id,
              ((uint64_t)a[0] << 32 | a[1]) * 1000000000u + a[2], a[3], a[6]);
    if (op == 2) fprintf(f, "%" PRIu64 " %d discarded %u %u\n", t, c->n, o->ref, id);
    break;
  case T_TOPLEVEL:
    if (op == 0 && n >= 2) fprintf(f, "%" PRIu64 " %d tconf %d %d\n", t, c->n, (int32_t)a[0], (int32_t)a[1]);
    break;
  case T_XDG_SURFACE:
    if (op == 0 && n >= 1) fprintf(f, "%" PRIu64 " %d sconf %u %u\n", t, c->n, o->ref, a[0]);
    break;
  }
}

/* Parse what whole messages the stream holds; keep the rest. */
static void parse(struct client *c, struct stream *s, bool up, uint64_t t, const uint8_t *data, size_t len) {
  if (s->len + len > s->cap) {
    s->cap = (s->len + len) * 2;
    s->buf = realloc(s->buf, s->cap);
  }
  memcpy(s->buf + s->len, data, len);
  s->len += len;
  size_t at = 0;
  while (s->len - at >= 8) {
    uint32_t h[2];
    memcpy(h, s->buf + at, 8);
    uint32_t size = h[1] >> 16, op = h[1] & 0xffff;
    if (size < 8) { fprintf(logf_, "%" PRIu64 " %d badsize\n", t, c->n); s->len = 0; return; }
    if (s->len - at < size) break;
    uint32_t args[(size - 8) / 4 + 1];
    memcpy(args, s->buf + at + 8, size - 8);
    if (up) request(c, t, h[0], op, args, args + (size - 8) / 4);
    else event(c, t, h[0], op, args, args + (size - 8) / 4);
    at += size;
  }
  memmove(s->buf, s->buf + at, s->len - at);
  s->len -= at;
}

static void queue(struct stream *s, const uint8_t *data, size_t len, const int *fds, int nfds) {
  if (s->out_len + len > s->out_cap) {
    s->out_cap = (s->out_len + len) * 2 + 4096;
    s->out = realloc(s->out, s->out_cap);
  }
  memcpy(s->out + s->out_len, data, len);
  s->out_len += len;
  for (int i = 0; i < nfds; i++) {
    if (s->nfds < (int)(sizeof s->fds / sizeof *s->fds)) s->fds[s->nfds++] = fds[i];
    else close(fds[i]);
  }
}

/* Write what's waiting; descriptors go with the first bytes after they came
 * (a receiver queues them until a message takes them), at most 28 at once. */
static int flush_to(int fd, struct stream *s) {
  while (s->out_len > 0) {
    struct iovec iov = {s->out, s->out_len};
    struct msghdr m = {.msg_iov = &iov, .msg_iovlen = 1};
    char control[CMSG_SPACE(MAX_FDS * sizeof(int))];
    int n = s->nfds < MAX_FDS ? s->nfds : MAX_FDS;
    if (n > 0) {
      if (s->nfds > MAX_FDS) iov.iov_len = 1;  /* the rest of the descriptors go with the next bytes */
      memset(control, 0, sizeof control);
      m.msg_control = control;
      m.msg_controllen = CMSG_SPACE(n * sizeof(int));
      struct cmsghdr *cm = CMSG_FIRSTHDR(&m);
      cm->cmsg_level = SOL_SOCKET;
      cm->cmsg_type = SCM_RIGHTS;
      cm->cmsg_len = CMSG_LEN(n * sizeof(int));
      memcpy(CMSG_DATA(cm), s->fds, n * sizeof(int));
    }
    ssize_t w = sendmsg(fd, &m, MSG_NOSIGNAL | MSG_DONTWAIT);
    if (w < 0) {
      if (errno == EAGAIN || errno == EINTR) return 0;
      return -1;
    }
    for (int i = 0; i < n; i++) close(s->fds[i]);
    memmove(s->fds, s->fds + n, (s->nfds - n) * sizeof(int));
    s->nfds -= n;
    memmove(s->out, s->out + w, s->out_len - (size_t)w);
    s->out_len -= (size_t)w;
  }
  return 0;
}

/* Read a chunk from `from`, log it, queue it for `to`. */
static int relay(struct client *c, int from, int to, struct stream *s, bool up) {
  uint8_t data[65536];
  char control[CMSG_SPACE(MAX_FDS_IN * sizeof(int))];
  struct iovec iov = {data, sizeof data};
  struct msghdr m = {.msg_iov = &iov, .msg_iovlen = 1, .msg_control = control, .msg_controllen = sizeof control};
  ssize_t r = recvmsg(from, &m, MSG_CMSG_CLOEXEC | MSG_DONTWAIT);
  uint64_t t = now_ns();
  if (r < 0) return (errno == EAGAIN || errno == EINTR) ? 0 : -1;
  if (r == 0) return -1;
  if (m.msg_flags & MSG_CTRUNC) fprintf(logf_, "%" PRIu64 " %d ctrunc\n", t, c->n);
  int fds[MAX_FDS_IN], nfds = 0;
  for (struct cmsghdr *cm = CMSG_FIRSTHDR(&m); cm; cm = CMSG_NXTHDR(&m, cm)) {
    if (cm->cmsg_level == SOL_SOCKET && cm->cmsg_type == SCM_RIGHTS) {
      int k = (int)((cm->cmsg_len - CMSG_LEN(0)) / sizeof(int));
      for (int i = 0; i < k && nfds < MAX_FDS_IN; i++) memcpy(&fds[nfds++], CMSG_DATA(cm) + i * sizeof(int), sizeof(int));
    }
  }
  queue(s, data, (size_t)r, fds, nfds);
  if (flush_to(to, s) < 0) return -1;
  parse(c, s, up, t, data, (size_t)r);
  return 0;
}

static void drop(int i) {
  struct client *c = clients[i];
  fprintf(logf_, "%" PRIu64 " %d gone\n", now_ns(), c->n);
  close(c->cfd);
  close(c->sfd);
  for (int k = 0; k < c->up.nfds; k++) close(c->up.fds[k]);
  for (int k = 0; k < c->down.nfds; k++) close(c->down.fds[k]);
  free(c->up.buf); free(c->down.buf); free(c->up.out); free(c->down.out);
  free(c->ids); free(c->server_ids);
  free(c);
  clients[i] = NULL;
}

static void on_signal(int s) { (void)s; stop = 1; }

int main(int argc, char **argv) {
  if (argc != 3) { fprintf(stderr, "usage: wltrace LISTEN_NAME LOGFILE\n"); return 2; }
  const char *rt = getenv("XDG_RUNTIME_DIR"), *upstream = getenv("WAYLAND_DISPLAY");
  if (!rt || !upstream) { fprintf(stderr, "wltrace: XDG_RUNTIME_DIR and WAYLAND_DISPLAY needed\n"); return 2; }
  logf_ = fopen(argv[2], "w");
  if (!logf_) { perror("wltrace: log"); return 2; }
  static char logbuf[1 << 20];
  setvbuf(logf_, logbuf, _IOFBF, sizeof logbuf);
  struct sockaddr_un listen_addr = {.sun_family = AF_UNIX}, up_addr = {.sun_family = AF_UNIX};
  snprintf(listen_addr.sun_path, sizeof listen_addr.sun_path, "%s/%s", rt, argv[1]);
  if (upstream[0] == '/') snprintf(up_addr.sun_path, sizeof up_addr.sun_path, "%s", upstream);
  else snprintf(up_addr.sun_path, sizeof up_addr.sun_path, "%s/%s", rt, upstream);
  unlink(listen_addr.sun_path);
  int lfd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  if (lfd < 0 || bind(lfd, (struct sockaddr *)&listen_addr, sizeof listen_addr) || listen(lfd, 16)) {
    perror("wltrace: listen");
    return 2;
  }
  struct sigaction sa = {.sa_handler = on_signal};
  sigaction(SIGTERM, &sa, NULL);
  sigaction(SIGINT, &sa, NULL);
  fprintf(logf_, "%" PRIu64 " 0 start %s %s\n", now_ns(), listen_addr.sun_path, up_addr.sun_path);
  fflush(logf_);
  while (!stop) {
    struct pollfd p[1 + 2 * MAX_CLIENTS];
    int who[1 + 2 * MAX_CLIENTS], np = 0;
    p[np] = (struct pollfd){lfd, POLLIN, 0}; who[np++] = -1;
    for (int i = 0; i < MAX_CLIENTS; i++) {
      struct client *c = clients[i];
      if (!c) continue;
      p[np] = (struct pollfd){c->cfd, POLLIN | (c->down.out_len ? POLLOUT : 0), 0}; who[np++] = 2 * i;
      p[np] = (struct pollfd){c->sfd, POLLIN | (c->up.out_len ? POLLOUT : 0), 0}; who[np++] = 2 * i + 1;
    }
    int r = poll(p, (nfds_t)np, 200);
    if (r < 0) { if (errno == EINTR) continue; perror("wltrace: poll"); break; }
    if (r == 0) { fflush(logf_); continue; }
    for (int k = 0; k < np; k++) {
      if (!p[k].revents) continue;
      if (who[k] < 0) {
        int cfd = accept4(lfd, NULL, NULL, SOCK_CLOEXEC);
        if (cfd < 0) continue;
        int sfd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if (sfd < 0 || connect(sfd, (struct sockaddr *)&up_addr, sizeof up_addr)) {
          perror("wltrace: connect upstream");
          close(cfd);
          if (sfd >= 0) close(sfd);
          continue;
        }
        int slot = -1;
        for (int i = 0; i < MAX_CLIENTS; i++) if (!clients[i]) { slot = i; break; }
        if (slot < 0) { close(cfd); close(sfd); continue; }
        struct client *c = calloc(1, sizeof *c);
        c->cfd = cfd; c->sfd = sfd; c->n = next_client++;
        c->ids = calloc(CLIENT_IDS, sizeof *c->ids);
        c->server_ids = calloc(SERVER_IDS, sizeof *c->server_ids);
        set(c, 1, T_DISPLAY, 0);
        clients[slot] = c;
        fprintf(logf_, "%" PRIu64 " %d connect\n", now_ns(), c->n);
        continue;
      }
      int i = who[k] / 2;
      struct client *c = clients[i];
      if (!c) continue;
      bool client_side = who[k] % 2 == 0;
      int rc = 0;
      if (p[k].revents & POLLOUT) rc = client_side ? flush_to(c->cfd, &c->down) : flush_to(c->sfd, &c->up);
      if (rc == 0 && (p[k].revents & POLLIN))
        rc = client_side ? relay(c, c->cfd, c->sfd, &c->up, true) : relay(c, c->sfd, c->cfd, &c->down, false);
      if (rc < 0 || (p[k].revents & (POLLHUP | POLLERR) && !(p[k].revents & POLLIN))) drop(i);
    }
  }
  for (int i = 0; i < MAX_CLIENTS; i++) if (clients[i]) drop(i);
  fflush(logf_);
  unlink(listen_addr.sun_path);
  return 0;
}
