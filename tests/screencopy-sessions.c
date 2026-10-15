// SPDX-License-Identifier: GPL-3.0-or-later
// A screencopy client for the screenshare-sessions check. It asks Hyprland for
// frames one at a time over one connection, as xdg-desktop-portal-hyprland
// does, then optionally stays connected without asking for more.
//
//   screencopy-sessions region OUTPUT X Y W H FRAMES [--hold SECONDS]
//   screencopy-sessions output OUTPUT FRAMES [--hold SECONDS]
//   screencopy-sessions regions OUTPUT COUNT [--hold SECONDS]   COUNT distinct regions, one frame each
//   screencopy-sessions toplevel HANDLE FRAMES [--hold SECONDS]
//
// Exits non-zero if the compositor fails any frame.
#define _GNU_SOURCE
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>

#include "hyprland-toplevel-export-v1-client-protocol.h"
#include "wlr-screencopy-unstable-v1-client-protocol.h"

struct output {
    struct wl_output *output;
    char name[64];
};

static struct wl_display *display;
static struct wl_shm *shm;
static struct zwlr_screencopy_manager_v1 *screencopy;
static struct hyprland_toplevel_export_manager_v1 *toplevel_export;
static struct output outputs[16];
static int output_count;

struct frame {
    uint32_t format, width, height, stride;
    bool buffer_info, buffer_done, ready, failed;
};

static void die(const char *message) {
    fprintf(stderr, "screencopy-sessions: %s\n", message);
    exit(1);
}

static void output_geometry(void *data, struct wl_output *o, int32_t x, int32_t y, int32_t pw, int32_t ph, int32_t subpixel,
                            const char *make, const char *model, int32_t transform) {}
static void output_mode(void *data, struct wl_output *o, uint32_t flags, int32_t w, int32_t h, int32_t refresh) {}
static void output_done(void *data, struct wl_output *o) {}
static void output_scale(void *data, struct wl_output *o, int32_t factor) {}
static void output_name(void *data, struct wl_output *o, const char *name) {
    struct output *out = data;
    snprintf(out->name, sizeof(out->name), "%s", name);
}
static void output_description(void *data, struct wl_output *o, const char *description) {}

static const struct wl_output_listener output_listener = {
    .geometry = output_geometry,
    .mode = output_mode,
    .done = output_done,
    .scale = output_scale,
    .name = output_name,
    .description = output_description,
};

static void global(void *data, struct wl_registry *registry, uint32_t name, const char *interface, uint32_t version) {
    if (strcmp(interface, wl_shm_interface.name) == 0) {
        shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, zwlr_screencopy_manager_v1_interface.name) == 0 && version >= 3) {
        screencopy = wl_registry_bind(registry, name, &zwlr_screencopy_manager_v1_interface, 3);
    } else if (strcmp(interface, hyprland_toplevel_export_manager_v1_interface.name) == 0) {
        toplevel_export = wl_registry_bind(registry, name, &hyprland_toplevel_export_manager_v1_interface, 1);
    } else if (strcmp(interface, wl_output_interface.name) == 0 && version >= 4 &&
               output_count < (int)(sizeof(outputs) / sizeof(outputs[0]))) {
        struct output *out = &outputs[output_count++];
        out->output = wl_registry_bind(registry, name, &wl_output_interface, 4);
        wl_output_add_listener(out->output, &output_listener, out);
    }
}

static void global_remove(void *data, struct wl_registry *registry, uint32_t name) {}

static const struct wl_registry_listener registry_listener = {.global = global, .global_remove = global_remove};

static void on_buffer(struct frame *f, uint32_t format, uint32_t width, uint32_t height, uint32_t stride) {
    f->format = format;
    f->width = width;
    f->height = height;
    f->stride = stride;
    f->buffer_info = true;
}

static void sc_buffer(void *data, struct zwlr_screencopy_frame_v1 *fr, uint32_t format, uint32_t w, uint32_t h, uint32_t stride) {
    on_buffer(data, format, w, h, stride);
}
static void sc_flags(void *data, struct zwlr_screencopy_frame_v1 *fr, uint32_t flags) {}
static void sc_ready(void *data, struct zwlr_screencopy_frame_v1 *fr, uint32_t hi, uint32_t lo, uint32_t ns) {
    ((struct frame *)data)->ready = true;
}
static void sc_failed(void *data, struct zwlr_screencopy_frame_v1 *fr) { ((struct frame *)data)->failed = true; }
static void sc_damage(void *data, struct zwlr_screencopy_frame_v1 *fr, uint32_t x, uint32_t y, uint32_t w, uint32_t h) {}
static void sc_dmabuf(void *data, struct zwlr_screencopy_frame_v1 *fr, uint32_t format, uint32_t w, uint32_t h) {}
static void sc_buffer_done(void *data, struct zwlr_screencopy_frame_v1 *fr) { ((struct frame *)data)->buffer_done = true; }

static const struct zwlr_screencopy_frame_v1_listener screencopy_listener = {
    .buffer = sc_buffer,
    .flags = sc_flags,
    .ready = sc_ready,
    .failed = sc_failed,
    .damage = sc_damage,
    .linux_dmabuf = sc_dmabuf,
    .buffer_done = sc_buffer_done,
};

static void te_buffer(void *data, struct hyprland_toplevel_export_frame_v1 *fr, uint32_t format, uint32_t w, uint32_t h, uint32_t stride) {
    on_buffer(data, format, w, h, stride);
}
static void te_damage(void *data, struct hyprland_toplevel_export_frame_v1 *fr, uint32_t x, uint32_t y, uint32_t w, uint32_t h) {}
static void te_flags(void *data, struct hyprland_toplevel_export_frame_v1 *fr, uint32_t flags) {}
static void te_ready(void *data, struct hyprland_toplevel_export_frame_v1 *fr, uint32_t hi, uint32_t lo, uint32_t ns) {
    ((struct frame *)data)->ready = true;
}
static void te_failed(void *data, struct hyprland_toplevel_export_frame_v1 *fr) { ((struct frame *)data)->failed = true; }
static void te_dmabuf(void *data, struct hyprland_toplevel_export_frame_v1 *fr, uint32_t format, uint32_t w, uint32_t h) {}
static void te_buffer_done(void *data, struct hyprland_toplevel_export_frame_v1 *fr) { ((struct frame *)data)->buffer_done = true; }

static const struct hyprland_toplevel_export_frame_v1_listener toplevel_listener = {
    .buffer = te_buffer,
    .damage = te_damage,
    .flags = te_flags,
    .ready = te_ready,
    .failed = te_failed,
    .linux_dmabuf = te_dmabuf,
    .buffer_done = te_buffer_done,
};

static struct wl_buffer *shm_buffer(const struct frame *f) {
    const size_t size = (size_t)f->stride * f->height;
    int fd = memfd_create("screencopy-sessions", MFD_CLOEXEC);
    if (fd < 0 || ftruncate(fd, (off_t)size) < 0)
        die("cannot allocate a buffer");
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, (int32_t)size);
    struct wl_buffer *buffer = wl_shm_pool_create_buffer(pool, 0, (int32_t)f->width, (int32_t)f->height, (int32_t)f->stride, f->format);
    wl_shm_pool_destroy(pool);
    close(fd);
    return buffer;
}

static void dispatch_until(const bool *a, const bool *b) {
    while (!*a && !*b)
        if (wl_display_dispatch(display) < 0)
            die("lost the compositor");
}

// One frame of an output, or of a region of it when w > 0.
static void capture_output(struct wl_output *output, int32_t x, int32_t y, int32_t w, int32_t h) {
    struct frame f = {0};
    struct zwlr_screencopy_frame_v1 *frame = w > 0 ? zwlr_screencopy_manager_v1_capture_output_region(screencopy, 0, output, x, y, w, h)
                                                   : zwlr_screencopy_manager_v1_capture_output(screencopy, 0, output);
    zwlr_screencopy_frame_v1_add_listener(frame, &screencopy_listener, &f);
    dispatch_until(&f.buffer_done, &f.failed);
    if (f.failed || !f.buffer_info)
        die("the compositor failed a frame before copying");
    struct wl_buffer *buffer = shm_buffer(&f);
    zwlr_screencopy_frame_v1_copy(frame, buffer);
    dispatch_until(&f.ready, &f.failed);
    if (f.failed)
        die("the compositor failed a frame");
    zwlr_screencopy_frame_v1_destroy(frame);
    wl_buffer_destroy(buffer);
}

static void capture_toplevel(uint32_t handle) {
    struct frame f = {0};
    struct hyprland_toplevel_export_frame_v1 *frame = hyprland_toplevel_export_manager_v1_capture_toplevel(toplevel_export, 0, handle);
    hyprland_toplevel_export_frame_v1_add_listener(frame, &toplevel_listener, &f);
    dispatch_until(&f.buffer_done, &f.failed);
    if (f.failed || !f.buffer_info)
        die("the compositor failed a window frame before copying");
    struct wl_buffer *buffer = shm_buffer(&f);
    hyprland_toplevel_export_frame_v1_copy(frame, buffer, 1);
    dispatch_until(&f.ready, &f.failed);
    if (f.failed)
        die("the compositor failed a window frame");
    hyprland_toplevel_export_frame_v1_destroy(frame);
    wl_buffer_destroy(buffer);
}

static struct wl_output *find_output(const char *name) {
    for (int i = 0; i < output_count; i++)
        if (strcmp(outputs[i].name, name) == 0)
            return outputs[i].output;
    die("no such output");
    return NULL;
}

int main(int argc, char **argv) {
    int hold = 0;
    if (argc >= 3 && strcmp(argv[argc - 2], "--hold") == 0) {
        hold = atoi(argv[argc - 1]);
        argc -= 2;
    }
    if (argc < 3)
        die("usage: region OUTPUT X Y W H FRAMES | output OUTPUT FRAMES | regions OUTPUT COUNT | toplevel HANDLE FRAMES [--hold SECONDS]");

    display = wl_display_connect(NULL);
    if (!display)
        die("cannot connect to the compositor");
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    wl_display_roundtrip(display);
    wl_display_roundtrip(display); // output names
    if (!shm || !screencopy || !toplevel_export)
        die("the compositor lacks wl_shm, zwlr_screencopy_manager_v1 v3 or hyprland_toplevel_export_manager_v1");

    const char *mode = argv[1];
    if (strcmp(mode, "region") == 0 && argc == 8) {
        struct wl_output *output = find_output(argv[2]);
        for (int i = 0, frames = atoi(argv[7]); i < frames; i++)
            capture_output(output, atoi(argv[3]), atoi(argv[4]), atoi(argv[5]), atoi(argv[6]));
    } else if (strcmp(mode, "output") == 0 && argc == 4) {
        struct wl_output *output = find_output(argv[2]);
        for (int i = 0, frames = atoi(argv[3]); i < frames; i++)
            capture_output(output, 0, 0, 0, 0);
    } else if (strcmp(mode, "regions") == 0 && argc == 4) {
        struct wl_output *output = find_output(argv[2]);
        for (int i = 0, count = atoi(argv[3]); i < count; i++)
            capture_output(output, i, 0, 64, 32);
    } else if (strcmp(mode, "toplevel") == 0 && argc == 4) {
        const uint32_t handle = (uint32_t)strtoul(argv[2], NULL, 0);
        for (int i = 0, frames = atoi(argv[3]); i < frames; i++)
            capture_toplevel(handle);
    } else {
        die("unknown mode or wrong arguments");
    }

    printf("captured\n");
    fflush(stdout);
    if (hold > 0) {
        wl_display_flush(display);
        sleep((unsigned)hold);
        // still connected: answer pings and other events before leaving
        wl_display_roundtrip(display);
    }
    wl_display_disconnect(display);
    return 0;
}
