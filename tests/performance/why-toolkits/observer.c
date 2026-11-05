/* The why-toolkits observer, for sway (wlroots) only: docs/performance/why-toolkits.md.
 * Derived from tests/performance/terminals/frame-observer.c, without its
 * markers: it types on a private virtual keyboard, moves and scrolls a
 * private virtual pointer, and copies a region of the output each time the
 * output has new damage (wlr-screencopy copy_with_damage), reporting a hash
 * of the region's pixels. No hooks in the program watched: a keystroke's
 * response is the first copied frame whose region differs from before it.
 *
 * Commands on stdin, one per line:
 *   region X Y W H     the region to copy (output pixels); resets
 *   watch / unwatch    start / stop copying frames
 *   key CODE           press and release an evdev key
 *   motion X Y         move the pointer to X, Y (of the output's EXTENT)
 *   axis V             one vertical wheel event of V (fixed, logical px), as a wheel
 *   tick on|off        an 8 x 8 overlay at the output's bottom right that
 *                      changes every frame (layer shell), so sway composes
 *                      on its 60 Hz grid as a display's vblank would have it
 *                      (an idle headless output composes the moment a
 *                      client commits)
 *   calib on|off       report the box of pixels that changed in each copy
 *   quit
 * JSON lines on stdout: {"event":"frame","t":<compositor ns>,"recv":<ns>,
 * "hash":"<hex>","n":<copies>} for every copy, {"event":"key","inject":<ns>}
 * etc. Times are CLOCK_MONOTONIC, the clock wlroots stamps frames with. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include "screencopy-client.h"
#include "virtual-keyboard-client.h"
#include "virtual-pointer-client.h"
#include "layer-shell-client.h"

static struct wl_display *display;
static struct wl_shm *shm;
static struct wl_output *output;
static struct wl_seat *seat;
static struct zwlr_screencopy_manager_v1 *copy_manager;
static struct zwp_virtual_keyboard_manager_v1 *key_manager;
static struct zwlr_virtual_pointer_manager_v1 *pointer_manager;
static struct wl_compositor *compositor;
static struct zwlr_layer_shell_v1 *layer_shell;
static struct wl_surface *tick_surface;
static struct zwlr_layer_surface_v1 *tick_layer;
static struct wl_buffer *tick_buffers[2];
static bool ticking, tick_configured, calib;
static uint64_t ticks;
static uint8_t *previous;
static struct zwp_virtual_keyboard_v1 *keyboard;
static struct zwlr_virtual_pointer_v1 *pointer;
static struct zwlr_screencopy_frame_v1 *frame;
static struct wl_buffer *buffer;
static uint8_t *pixels;
static size_t allocation;
static uint32_t width, height, stride, format;
static int rx, ry, rw = 0, rh = 0;
static bool watching, quit;
static uint64_t copies, request_ns;
static int extent_w = 1920, extent_h = 1080;

static uint64_t now_ns(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * 1000000000u + (uint64_t)t.tv_nsec;
}
static void fail(const char *m) { fprintf(stderr, "observer: %s (errno %d)\n", m, errno); exit(2); }

static uint64_t hash_region(void) {
  /* FNV-1a over 64-bit words of each row. */
  uint64_t h = 1469598103934665603ull;
  size_t row = (size_t)width * 4;
  for (uint32_t y = 0; y < height; y++) {
    const uint64_t *p = (const uint64_t *)(pixels + (size_t)y * stride);
    for (size_t i = 0; i < row / 8; i++) { h ^= p[i]; h *= 1099511628211ull; }
  }
  return h;
}

static void request_copy(void);

/* The ticker: two 8 x 8 buffers, swapped at every frame callback. */
static void tick_frame(void *d, struct wl_callback *cb, uint32_t t);
static const struct wl_callback_listener tick_listener = {.done = tick_frame};
static void tick_commit(void) {
  struct wl_callback *cb = wl_surface_frame(tick_surface);
  wl_callback_add_listener(cb, &tick_listener, NULL);
  wl_surface_attach(tick_surface, tick_buffers[ticks & 1], 0, 0);
  wl_surface_damage_buffer(tick_surface, 0, 0, 8, 8);
  wl_surface_commit(tick_surface);
  ++ticks;
}
static void tick_frame(void *d, struct wl_callback *cb, uint32_t t) {
  (void)d; (void)t;
  wl_callback_destroy(cb);
  if (ticking) tick_commit();
}
static void tick_configure(void *d, struct zwlr_layer_surface_v1 *l, uint32_t serial, uint32_t w, uint32_t h) {
  (void)d; (void)w; (void)h;
  zwlr_layer_surface_v1_ack_configure(l, serial);
  if (!tick_configured) { tick_configured = true; if (ticking) tick_commit(); }
}
static void tick_closed(void *d, struct zwlr_layer_surface_v1 *l) { (void)d; (void)l; }
static const struct zwlr_layer_surface_v1_listener tick_layer_listener = {.configure = tick_configure, .closed = tick_closed};
static void tick_start(void) {
  if (!tick_surface) {
    int fd = memfd_create("tk-ticker", MFD_CLOEXEC);
    if (fd < 0 || ftruncate(fd, 2 * 8 * 8 * 4)) fail("ticker shm");
    uint32_t *px = mmap(NULL, 2 * 8 * 8 * 4, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    for (int i = 0; i < 64; i++) { px[i] = 0xff303030; px[64 + i] = 0xff383838; }
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, 2 * 8 * 8 * 4);
    for (int i = 0; i < 2; i++) tick_buffers[i] = wl_shm_pool_create_buffer(pool, i * 256, 8, 8, 32, WL_SHM_FORMAT_XRGB8888);
    wl_shm_pool_destroy(pool); close(fd);
    tick_surface = wl_compositor_create_surface(compositor);
    tick_layer = zwlr_layer_shell_v1_get_layer_surface(layer_shell, tick_surface, output, ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, "tk-ticker");
    zwlr_layer_surface_v1_add_listener(tick_layer, &tick_layer_listener, NULL);
    zwlr_layer_surface_v1_set_size(tick_layer, 8, 8);
    zwlr_layer_surface_v1_set_anchor(tick_layer, ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM | ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);
    zwlr_layer_surface_v1_set_keyboard_interactivity(tick_layer, 0);
    wl_surface_commit(tick_surface);
    ticking = true;
    return;
  }
  if (!ticking) { ticking = true; if (tick_configured) tick_commit(); }
}

static void offered(void *d, struct zwlr_screencopy_frame_v1 *f, uint32_t fmt, uint32_t w, uint32_t h, uint32_t s) {
  (void)d; (void)f;
  if (buffer && (w != width || h != height || s != stride || fmt != format)) {
    wl_buffer_destroy(buffer); munmap(pixels, allocation); buffer = NULL;
    free(previous); previous = NULL;
  }
  if (!buffer) {
    width = w; height = h; stride = s; format = fmt; allocation = (size_t)s * h;
    int fd = memfd_create("tk-observer", MFD_CLOEXEC);
    if (fd < 0 || ftruncate(fd, (off_t)allocation)) fail("shm");
    pixels = mmap(NULL, allocation, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (pixels == MAP_FAILED) fail("mmap");
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, (int)allocation);
    buffer = wl_shm_pool_create_buffer(pool, 0, (int)w, (int)h, (int)s, fmt);
    wl_shm_pool_destroy(pool); close(fd);
  }
}
static void flags_ev(void *d, struct zwlr_screencopy_frame_v1 *f, uint32_t fl) { (void)d; (void)f; (void)fl; }
static void ready(void *d, struct zwlr_screencopy_frame_v1 *f, uint32_t hi, uint32_t lo, uint32_t ns) {
  (void)d;
  uint64_t recv = now_ns();
  uint64_t t = (((uint64_t)hi << 32) | lo) * 1000000000ull + ns;
  uint64_t h = hash_region();
  ++copies;
  int bx0 = -1, by0 = -1, bx1 = -1, by1 = -1;
  if (calib) {
    if (!previous) previous = calloc(1, allocation);
    for (uint32_t y = 0; y < height; y++) {
      const uint32_t *a = (const uint32_t *)(pixels + (size_t)y * stride), *b = (const uint32_t *)(previous + (size_t)y * stride);
      for (uint32_t x = 0; x < width; x++) {
        if (a[x] != b[x]) {
          if (bx0 < 0 || (int)x < bx0) bx0 = (int)x;
          if ((int)x > bx1) bx1 = (int)x;
          if (by0 < 0) by0 = (int)y;
          by1 = (int)y;
        }
      }
    }
    memcpy(previous, pixels, allocation);
  }
  printf("{\"event\":\"frame\",\"t\":%" PRIu64 ",\"recv\":%" PRIu64 ",\"req\":%" PRIu64 ",\"hash\":\"%016" PRIx64 "\",\"n\":%" PRIu64
         ",\"box\":[%d,%d,%d,%d],\"ticks\":%" PRIu64 "}\n",
         t, recv, request_ns, h, copies, bx0, by0, bx1, by1, ticks);
  zwlr_screencopy_frame_v1_destroy(f);
  frame = NULL;
  if (watching) request_copy();
}
static void failed(void *d, struct zwlr_screencopy_frame_v1 *f) { (void)d; zwlr_screencopy_frame_v1_destroy(f); frame = NULL; fail("screencopy failed"); }
static void damage(void *d, struct zwlr_screencopy_frame_v1 *f, uint32_t x, uint32_t y, uint32_t w, uint32_t h) { (void)d; (void)f; (void)x; (void)y; (void)w; (void)h; }
static void dmabuf(void *d, struct zwlr_screencopy_frame_v1 *f, uint32_t fmt, uint32_t w, uint32_t h) { (void)d; (void)f; (void)fmt; (void)w; (void)h; }
static void buffer_done(void *d, struct zwlr_screencopy_frame_v1 *f) {
  (void)d;
  if (!buffer) fail("no shm format offered");
  zwlr_screencopy_frame_v1_copy_with_damage(f, buffer);
}
static const struct zwlr_screencopy_frame_v1_listener copy_listener = {
  .buffer = offered, .flags = flags_ev, .ready = ready, .failed = failed,
  .damage = damage, .linux_dmabuf = dmabuf, .buffer_done = buffer_done,
};

static void request_copy(void) {
  if (frame) return;
  request_ns = now_ns();
  if (rw > 0 && rh > 0) frame = zwlr_screencopy_manager_v1_capture_output_region(copy_manager, 0, output, rx, ry, rw, rh);
  else frame = zwlr_screencopy_manager_v1_capture_output(copy_manager, 0, output);
  zwlr_screencopy_frame_v1_add_listener(frame, &copy_listener, NULL);
}

static void global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t version) {
  (void)d;
  if (!strcmp(iface, "wl_shm")) shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
  else if (!strcmp(iface, "wl_output") && !output) output = wl_registry_bind(r, name, &wl_output_interface, 1);
  else if (!strcmp(iface, "wl_seat") && !seat) seat = wl_registry_bind(r, name, &wl_seat_interface, 1);
  else if (!strcmp(iface, "zwlr_screencopy_manager_v1") && version >= 3)
    copy_manager = wl_registry_bind(r, name, &zwlr_screencopy_manager_v1_interface, 3);
  else if (!strcmp(iface, "zwp_virtual_keyboard_manager_v1"))
    key_manager = wl_registry_bind(r, name, &zwp_virtual_keyboard_manager_v1_interface, 1);
  else if (!strcmp(iface, "wl_compositor")) compositor = wl_registry_bind(r, name, &wl_compositor_interface, 4);
  else if (!strcmp(iface, "zwlr_layer_shell_v1")) layer_shell = wl_registry_bind(r, name, &zwlr_layer_shell_v1_interface, 1);
  else if (!strcmp(iface, "zwlr_virtual_pointer_manager_v1"))
    pointer_manager = wl_registry_bind(r, name, &zwlr_virtual_pointer_manager_v1_interface, version >= 2 ? 2 : 1);
}
static void removed(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener registry_listener = {.global = global, .global_remove = removed};

static void command(char *line) {
  int a, b, c, d;
  double v;
  uint64_t t0 = now_ns();
  uint32_t ms = (uint32_t)(t0 / 1000000);
  if (sscanf(line, "region %d %d %d %d", &a, &b, &c, &d) == 4) { rx = a; ry = b; rw = c; rh = d; }
  else if (sscanf(line, "extent %d %d", &a, &b) == 2) { extent_w = a; extent_h = b; }
  else if (!strcmp(line, "watch")) { watching = true; request_copy(); }
  else if (!strcmp(line, "tick on")) tick_start();
  else if (!strcmp(line, "tick off")) ticking = false;
  else if (!strcmp(line, "calib on")) calib = true;
  else if (!strcmp(line, "calib off")) calib = false;
  else if (!strcmp(line, "unwatch")) watching = false;
  else if (sscanf(line, "key %d", &a) == 1) {
    zwp_virtual_keyboard_v1_key(keyboard, ms, (uint32_t)a, WL_KEYBOARD_KEY_STATE_PRESSED);
    zwp_virtual_keyboard_v1_key(keyboard, ms, (uint32_t)a, WL_KEYBOARD_KEY_STATE_RELEASED);
    if (wl_display_flush(display) < 0 && errno != EAGAIN) fail("flush");
    printf("{\"event\":\"key\",\"code\":%d,\"inject\":%" PRIu64 ",\"flushed\":%" PRIu64 "}\n", a, t0, now_ns());
    return;
  } else if (sscanf(line, "motion %d %d", &a, &b) == 2) {
    zwlr_virtual_pointer_v1_motion_absolute(pointer, ms, (uint32_t)a, (uint32_t)b, (uint32_t)extent_w, (uint32_t)extent_h);
    zwlr_virtual_pointer_v1_frame(pointer);
    if (wl_display_flush(display) < 0 && errno != EAGAIN) fail("flush");
    printf("{\"event\":\"motion\",\"x\":%d,\"y\":%d,\"inject\":%" PRIu64 "}\n", a, b, t0);
    return;
  } else if (sscanf(line, "axis %lf", &v) == 1) {
    zwlr_virtual_pointer_v1_axis_source(pointer, WL_POINTER_AXIS_SOURCE_WHEEL);
    zwlr_virtual_pointer_v1_axis_discrete(pointer, ms, WL_POINTER_AXIS_VERTICAL_SCROLL, wl_fixed_from_double(v), v > 0 ? 1 : -1);
    zwlr_virtual_pointer_v1_frame(pointer);
    if (wl_display_flush(display) < 0 && errno != EAGAIN) fail("flush");
    printf("{\"event\":\"axis\",\"value\":%g,\"inject\":%" PRIu64 "}\n", v, t0);
    return;
  } else if (!strcmp(line, "quit")) quit = true;
  else fail("unknown command");
  printf("{\"event\":\"ok\",\"t\":%" PRIu64 "}\n", t0);
}

int main(void) {
  setvbuf(stdout, NULL, _IOLBF, 0);
  display = wl_display_connect(NULL);
  if (!display) fail("connect");
  struct wl_registry *registry = wl_display_get_registry(display);
  wl_registry_add_listener(registry, &registry_listener, NULL);
  if (wl_display_roundtrip(display) < 0 || !shm || !output || !seat || !copy_manager || !key_manager || !pointer_manager || !compositor || !layer_shell)
    fail("protocols missing");
  struct xkb_context *ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
  struct xkb_rule_names names = {.layout = "us"};
  struct xkb_keymap *km = xkb_keymap_new_from_names(ctx, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
  char *text = km ? xkb_keymap_get_as_string(km, XKB_KEYMAP_FORMAT_TEXT_V1) : NULL;
  if (!text) fail("keymap");
  int fd = memfd_create("tk-keymap", MFD_CLOEXEC);
  size_t len = strlen(text) + 1;
  if (fd < 0 || write(fd, text, len) != (ssize_t)len) fail("keymap fd");
  keyboard = zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(key_manager, seat);
  zwp_virtual_keyboard_v1_keymap(keyboard, WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1, fd, (uint32_t)len);
  close(fd); free(text); xkb_keymap_unref(km); xkb_context_unref(ctx);
  pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer(pointer_manager, seat);
  if (wl_display_roundtrip(display) < 0) fail("roundtrip");
  printf("{\"event\":\"ready\",\"t\":%" PRIu64 "}\n", now_ns());
  int fl = fcntl(0, F_GETFL);
  fcntl(0, F_SETFL, fl | O_NONBLOCK);
  char in[4096];
  size_t used = 0;
  while (!quit) {
    while (wl_display_prepare_read(display) != 0)
      if (wl_display_dispatch_pending(display) < 0) fail("dispatch");
    if (wl_display_flush(display) < 0 && errno != EAGAIN) { wl_display_cancel_read(display); fail("flush"); }
    struct pollfd p[2] = {{wl_display_get_fd(display), POLLIN, 0}, {0, POLLIN, 0}};
    if (poll(p, 2, -1) < 0) { wl_display_cancel_read(display); if (errno == EINTR) continue; fail("poll"); }
    if (p[0].revents & POLLIN) { if (wl_display_read_events(display) < 0) fail("read"); }
    else wl_display_cancel_read(display);
    if (wl_display_dispatch_pending(display) < 0) fail("dispatch");
    if (p[0].revents & (POLLERR | POLLHUP)) fail("disconnected");
    if (p[1].revents & (POLLIN | POLLHUP)) {
      ssize_t n = read(0, in + used, sizeof in - used - 1);
      if (n == 0) break;
      if (n < 0) { if (errno == EAGAIN || errno == EINTR) continue; fail("stdin"); }
      used += (size_t)n; in[used] = 0;
      char *nl;
      while ((nl = memchr(in, '\n', used))) {
        *nl = 0;
        command(in);
        size_t k = (size_t)(nl - in) + 1;
        memmove(in, in + k, used - k);
        used -= k;
      }
    }
  }
  return 0;
}
