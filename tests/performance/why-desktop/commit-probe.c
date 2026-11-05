// Timer-driven update probe for docs/performance/why-desktop.md.
//
// The desktop comparison's wl-probe (tests/performance/desktops/wl-probe.c)
// flips its window on key presses inside the VMs. On a headless compositor
// on the host there is no keyboard to press, so this one flips the same
// 800x600 window (two wl_shm buffers filled once, full damage) on a timer,
// 120-250 ms apart at random, and records each commit's presentation, as
// one key's update would be. Its records answer "what does the compositor
// do for one update to an open window", not the vblank phase (a headless
// output has no real vblank).
//
//   commit-probe COUNT OUT.tsv STATUS [HOLD_S]
//       After the window is mapped and presented: COUNT updates, then the
//       window stays up HOLD_S seconds (for a window-move phase), then exits.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#include <wayland-client.h>

#include "presentation-time-client-protocol.h"
#include "xdg-shell-client-protocol.h"

#define DEFAULT_W 800
#define DEFAULT_H 600
#define NBUF 6
#define MAXREC 200000

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

struct buffer {
    struct wl_buffer *wl;
    void *data;
    size_t size;
    int colour;
    bool busy;
};

struct record {
    uint64_t key_ms;    // wl_keyboard.key time (ms, compositor's clock)
    uint64_t recv_ns;   // probe received the key / frame callback
    uint64_t commit_ns; // probe committed the frame
    uint64_t present_ns;
    uint32_t refresh_ns;
    uint32_t flags;
    uint64_t seq;
    int state; // 0 pending, 1 presented, 2 discarded
};

static struct wl_display *display;
static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *wm_base;
static struct wl_seat *seat;
static struct wl_keyboard *keyboard;
static struct wp_presentation *presentation;
static uint32_t clock_id = 0xffffffff;
static struct wl_surface *surface;
static struct xdg_surface *xsurface;
static struct xdg_toplevel *toplevel;
static struct buffer buffers[NBUF];
static int width = DEFAULT_W, height = DEFAULT_H;
static int pending_w, pending_h;
static bool configured, focused, first_presented, running = true;
static int colour;
static struct record *records;
static size_t nrec;
static size_t target;
static const char *status_path;
static bool pacing;
static uint64_t pacing_end;
static uint64_t focus_ns;

static void write_status(const char *text) {
    FILE *f = fopen(status_path, "w");
    if (f) {
        fprintf(f, "%s\n", text);
        fclose(f);
    }
}

static void buffer_release(void *data, struct wl_buffer *wl) {
    (void)wl;
    ((struct buffer *)data)->busy = false;
}
static const struct wl_buffer_listener buffer_listener = {.release = buffer_release};

static void destroy_buffers(void) {
    for (int i = 0; i < NBUF; i++) {
        if (buffers[i].wl) {
            wl_buffer_destroy(buffers[i].wl);
            munmap(buffers[i].data, buffers[i].size);
        }
        memset(&buffers[i], 0, sizeof buffers[i]);
    }
}

static void create_buffers(void) {
    destroy_buffers();
    int stride = width * 4;
    size_t size = (size_t)stride * height;
    for (int i = 0; i < NBUF; i++) {
        int fd = memfd_create("wl-probe", MFD_CLOEXEC);
        if (fd < 0 || ftruncate(fd, size) < 0) {
            perror("memfd");
            exit(1);
        }
        void *data = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        if (data == MAP_FAILED) {
            perror("mmap");
            exit(1);
        }
        int c = i % 2;
        // Indigo and a pale yellow: every pixel changes between them.
        uint32_t px = c ? 0xfff2e8a0u : 0xff1b2a6bu;
        uint32_t *p = data;
        for (size_t j = 0; j < size / 4; j++) p[j] = px;
        struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, size);
        buffers[i].wl = wl_shm_pool_create_buffer(pool, 0, width, height, stride, WL_SHM_FORMAT_XRGB8888);
        wl_shm_pool_destroy(pool);
        close(fd);
        buffers[i].data = data;
        buffers[i].size = size;
        buffers[i].colour = c;
        buffers[i].busy = false;
        wl_buffer_add_listener(buffers[i].wl, &buffer_listener, &buffers[i]);
    }
}

static struct buffer *next_buffer(int c) {
    for (int i = 0; i < NBUF; i++)
        if (buffers[i].colour == c && !buffers[i].busy) return &buffers[i];
    return NULL;
}

static void feedback_sync(void *d, struct wp_presentation_feedback *f, struct wl_output *o) {
    (void)d; (void)f; (void)o;
}
static void feedback_presented(void *data, struct wp_presentation_feedback *f, uint32_t sec_hi, uint32_t sec_lo,
                               uint32_t nsec, uint32_t refresh, uint32_t seq_hi, uint32_t seq_lo, uint32_t flags) {
    intptr_t idx = (intptr_t)data;
    uint64_t t = ((((uint64_t)sec_hi << 32) | sec_lo) * 1000000000ull) + nsec;
    if (idx >= 0) {
        records[idx].present_ns = t;
        records[idx].refresh_ns = refresh;
        records[idx].flags = flags;
        records[idx].seq = ((uint64_t)seq_hi << 32) | seq_lo;
        records[idx].state = 1;
    } else if (!first_presented) {
        first_presented = true;
    }
    wp_presentation_feedback_destroy(f);
}
static void feedback_discarded(void *data, struct wp_presentation_feedback *f) {
    intptr_t idx = (intptr_t)data;
    if (idx >= 0) records[idx].state = 2;
    else first_presented = true; // the window is up even if that frame was replaced
    wp_presentation_feedback_destroy(f);
}
static const struct wp_presentation_feedback_listener feedback_listener = {
    .sync_output = feedback_sync, .presented = feedback_presented, .discarded = feedback_discarded};

static const struct wl_callback_listener frame_listener;

// Attach the other colour, damage everything, ask for feedback, commit.
static bool flip(intptr_t idx, bool want_frame) {
    colour ^= 1;
    struct buffer *b = next_buffer(colour);
    if (!b) {
        colour ^= 1;
        return false;
    }
    b->busy = true;
    wl_surface_attach(surface, b->wl, 0, 0);
    wl_surface_damage_buffer(surface, 0, 0, INT32_MAX, INT32_MAX);
    if (presentation) {
        struct wp_presentation_feedback *f = wp_presentation_feedback(presentation, surface);
        wp_presentation_feedback_add_listener(f, &feedback_listener, (void *)idx);
    }
    if (want_frame) {
        struct wl_callback *cb = wl_surface_frame(surface);
        wl_callback_add_listener(cb, &frame_listener, NULL);
    }
    if (idx >= 0) records[idx].commit_ns = now_ns();
    wl_surface_commit(surface);
    return true;
}

static void frame_done(void *data, struct wl_callback *cb, uint32_t time) {
    (void)data; (void)time;
    wl_callback_destroy(cb);
    if (!pacing || !running) return;
    uint64_t t = now_ns();
    if (t >= pacing_end || nrec >= MAXREC) {
        running = false;
        return;
    }
    size_t idx = nrec;
    memset(&records[idx], 0, sizeof records[idx]);
    records[idx].recv_ns = t;
    if (flip((intptr_t)idx, true)) {
        nrec++;
    } else {
        // No free buffer: repaint the current one and ask again.
        records[idx].state = 3;
        nrec++;
        struct wl_callback *again = wl_surface_frame(surface);
        wl_callback_add_listener(again, &frame_listener, NULL);
        wl_surface_damage_buffer(surface, 0, 0, INT32_MAX, INT32_MAX);
        wl_surface_commit(surface);
    }
}
static const struct wl_callback_listener frame_listener = {.done = frame_done};

static void pres_clock(void *d, struct wp_presentation *p, uint32_t id) {
    (void)d; (void)p;
    clock_id = id;
}
static const struct wp_presentation_listener presentation_listener = {.clock_id = pres_clock};

static void wm_ping(void *d, struct xdg_wm_base *b, uint32_t serial) {
    (void)d;
    xdg_wm_base_pong(b, serial);
}
static const struct xdg_wm_base_listener wm_listener = {.ping = wm_ping};

static void registry_global(void *d, struct wl_registry *r, uint32_t name, const char *iface, uint32_t version) {
    (void)d;
    if (!strcmp(iface, wl_compositor_interface.name))
        compositor = wl_registry_bind(r, name, &wl_compositor_interface, version < 4 ? version : 4);
    else if (!strcmp(iface, wl_shm_interface.name))
        shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
    else if (!strcmp(iface, xdg_wm_base_interface.name)) {
        wm_base = wl_registry_bind(r, name, &xdg_wm_base_interface, 1);
        xdg_wm_base_add_listener(wm_base, &wm_listener, NULL);
    } else if (!strcmp(iface, wp_presentation_interface.name)) {
        presentation = wl_registry_bind(r, name, &wp_presentation_interface, 1);
        wp_presentation_add_listener(presentation, &presentation_listener, NULL);
    }
}
static void registry_remove(void *d, struct wl_registry *r, uint32_t name) { (void)d; (void)r; (void)name; }
static const struct wl_registry_listener registry_listener = {.global = registry_global,
                                                              .global_remove = registry_remove};

static void xs_configure(void *d, struct xdg_surface *s, uint32_t serial) {
    (void)d;
    xdg_surface_ack_configure(s, serial);
    int w = pending_w > 0 ? pending_w : DEFAULT_W;
    int h = pending_h > 0 ? pending_h : DEFAULT_H;
    if (!configured || w != width || h != height) {
        width = w;
        height = h;
        create_buffers();
        configured = true;
        flip(-1, false);
    } else {
        wl_surface_commit(surface);
    }
}
static const struct xdg_surface_listener xs_listener = {.configure = xs_configure};

static void tl_configure(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *states) {
    (void)d; (void)t; (void)states;
    pending_w = w;
    pending_h = h;
}
static void tl_close(void *d, struct xdg_toplevel *t) {
    (void)d; (void)t;
    running = false;
}
static const struct xdg_toplevel_listener tl_listener = {.configure = tl_configure, .close = tl_close};

static void on_signal(int s) {
    (void)s;
    running = false;
}

static void dispatch_until(bool (*done)(void), uint64_t deadline_ns) {
    while (!done() && now_ns() < deadline_ns) {
        if (wl_display_dispatch_pending(display) < 0) break;
        wl_display_flush(display);
        struct pollfd p = {.fd = wl_display_get_fd(display), .events = POLLIN};
        if (wl_display_prepare_read(display) != 0) continue;
        int r = poll(&p, 1, 50);
        if (r > 0) {
            if (wl_display_read_events(display) < 0) break;
        } else {
            wl_display_cancel_read(display);
        }
    }
}
static bool is_ready(void) { return configured && first_presented && (focused || pacing) && presentation; }
static bool is_mapped(void) { return configured && first_presented; }
static bool is_done(void) { return !running; }
static bool all_feedback(void) {
    for (size_t i = 0; i < nrec; i++)
        if (records[i].state == 0) return false;
    return true;
}

static uint64_t rng = 0x9e3779b97f4a7c15ull;
static uint64_t next_rand(void) {
    rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;
    return rng;
}
static bool is_mapped_only(void) { return configured && first_presented && presentation; }

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr, "usage: %s COUNT OUT.tsv STATUS [HOLD_S]\n", argv[0]);
        return 2;
    }
    target = (size_t)strtol(argv[1], NULL, 10);
    const char *out = argv[2];
    status_path = argv[3];
    double hold = argc > 4 ? strtod(argv[4], NULL) : 0;
    records = calloc(MAXREC, sizeof *records);
    signal(SIGTERM, on_signal);
    signal(SIGINT, on_signal);
    write_status("starting");
    display = wl_display_connect(NULL);
    if (!display) { write_status("no-display"); return 1; }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    wl_display_roundtrip(display);
    wl_display_roundtrip(display);
    if (!compositor || !shm || !wm_base) { write_status("missing-globals"); return 1; }
    surface = wl_compositor_create_surface(compositor);
    xsurface = xdg_wm_base_get_xdg_surface(wm_base, surface);
    xdg_surface_add_listener(xsurface, &xs_listener, NULL);
    toplevel = xdg_surface_get_toplevel(xsurface);
    xdg_toplevel_add_listener(toplevel, &tl_listener, NULL);
    xdg_toplevel_set_title(toplevel, "commit-probe");
    xdg_toplevel_set_app_id(toplevel, "computer.sekka.commit-probe");
    wl_surface_commit(surface);
    uint64_t start = now_ns();
    dispatch_until(is_mapped_only, start + 30000000000ull);
    if (!is_mapped_only()) { write_status("not-ready"); return 1; }
    // Let the window's arrival settle before the updates.
    dispatch_until(is_done, now_ns() + 2000000000ull);
    char status[160];
    snprintf(status, sizeof status, "ready clock=%u size=%dx%d", clock_id, width, height);
    write_status(status);
    for (size_t i = 0; i < target && running; i++) {
        uint64_t wait = 120000000ull + next_rand() % 130000000ull;
        dispatch_until(is_done, now_ns() + wait);
        size_t idx = nrec++;
        memset(&records[idx], 0, sizeof records[idx]);
        records[idx].recv_ns = now_ns();
        if (!flip((intptr_t)idx, false)) records[idx].state = 3;
    }
    dispatch_until(all_feedback, now_ns() + 2000000000ull);
    FILE *f = fopen(out, "w");
    if (!f) { perror(out); return 1; }
    fprintf(f, "# clock_id=%u width=%d height=%d mode=timer\n", clock_id, width, height);
    fprintf(f, "idx\trecv_ns\tcommit_ns\tpresent_ns\trefresh_ns\tseq\tflags\tstate\n");
    for (size_t i = 0; i < nrec; i++) {
        struct record *r = &records[i];
        fprintf(f, "%zu\t%llu\t%llu\t%llu\t%u\t%llu\t%u\t%d\n", i, (unsigned long long)r->recv_ns,
                (unsigned long long)r->commit_ns, (unsigned long long)r->present_ns, r->refresh_ns,
                (unsigned long long)r->seq, r->flags, r->state);
    }
    fclose(f);
    write_status("updates-done");
    if (hold > 0 && running) dispatch_until(is_done, now_ns() + (uint64_t)(hold * 1e9));
    write_status("done");
    return 0;
}
