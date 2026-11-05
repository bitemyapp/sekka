/* Input for KWin, through its fake-input protocol (org_kde_kwin_fake_input),
 * for docs/performance/why-toolkits.md: KWin has neither wlr's virtual
 * keyboard nor its virtual pointer. KWin offers fake input only to programs
 * its desktop files allow, or to all with KWIN_WAYLAND_NO_PERMISSION_CHECKS=1
 * (set for these headless runs only).
 *
 * Commands on stdin, one per line: "key CODE" (an evdev key pressed and
 * released), "axis V" (vertical scroll by V), "motion X Y" (absolute),
 * "quit". Each is answered with a JSON line holding when it was sent
 * (CLOCK_MONOTONIC ns). */
#define _GNU_SOURCE
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>
#include "fake-input-client.h"

static struct org_kde_kwin_fake_input *fake;

static uint64_t now_ns(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * 1000000000u + (uint64_t)t.tv_nsec;
}

static void global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t version) {
  (void)d;
  if (!strcmp(iface, "org_kde_kwin_fake_input") && version >= 4)
    fake = wl_registry_bind(r, name, &org_kde_kwin_fake_input_interface, version < 5 ? version : 5);
}
static void removed(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener listener = {.global = global, .global_remove = removed};

int main(void) {
  setvbuf(stdout, NULL, _IOLBF, 0);
  struct wl_display *display = wl_display_connect(NULL);
  if (!display) { fprintf(stderr, "kwin-input: connect\n"); return 2; }
  struct wl_registry *registry = wl_display_get_registry(display);
  wl_registry_add_listener(registry, &listener, NULL);
  wl_display_roundtrip(display);
  if (!fake) { fprintf(stderr, "kwin-input: no org_kde_kwin_fake_input (v4+)\n"); return 2; }
  org_kde_kwin_fake_input_authenticate(fake, "why-toolkits", "benchmark input");
  wl_display_roundtrip(display);
  printf("{\"event\":\"ready\",\"t\":%" PRIu64 "}\n", now_ns());
  char line[256];
  while (fgets(line, sizeof line, stdin)) {
    int a, b;
    double v;
    uint64_t t = now_ns();
    if (sscanf(line, "key %d", &a) == 1) {
      org_kde_kwin_fake_input_keyboard_key(fake, (uint32_t)a, 1);
      org_kde_kwin_fake_input_keyboard_key(fake, (uint32_t)a, 0);
      wl_display_flush(display);
      printf("{\"event\":\"key\",\"code\":%d,\"inject\":%" PRIu64 "}\n", a, t);
    } else if (sscanf(line, "axis %lf", &v) == 1) {
      org_kde_kwin_fake_input_axis(fake, WL_POINTER_AXIS_VERTICAL_SCROLL, wl_fixed_from_double(v));
      wl_display_flush(display);
      printf("{\"event\":\"axis\",\"inject\":%" PRIu64 "}\n", t);
    } else if (sscanf(line, "motion %d %d", &a, &b) == 2) {
      org_kde_kwin_fake_input_pointer_motion_absolute(fake, wl_fixed_from_int(a), wl_fixed_from_int(b));
      wl_display_flush(display);
      printf("{\"event\":\"motion\",\"inject\":%" PRIu64 "}\n", t);
    } else if (!strncmp(line, "quit", 4)) {
      break;
    }
    if (wl_display_dispatch_pending(display) < 0) break;
  }
  wl_display_disconnect(display);
  return 0;
}
