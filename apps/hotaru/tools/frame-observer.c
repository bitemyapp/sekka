/* Private-compositor benchmark observer. No application hooks or host input.
 * Persistent screencopy, one SHM buffer and one capture in flight. The marker
 * uses background colors, so decoding does not depend on font rasterization.
 * Build with generated wlr-screencopy and virtual-keyboard protocol sources. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <limits.h>
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

enum { MARKER_CELLS = 76, MAX_BUFFER = 64 * 1024 * 1024 };
static const uint32_t guard[6] = {0xf71f35, 0x1fd7ef, 0xe7bd13, 0x2941e9, 0xf3f3f3, 0x111111};
static const uint32_t bit_color[2] = {0x20e050, 0xc028e0};
static struct wl_display *display;
static struct wl_shm *shm;
static struct wl_output *output;
static struct wl_seat *seat;
static struct zwlr_screencopy_manager_v1 *copy_manager;
static struct zwp_virtual_keyboard_manager_v1 *key_manager;
static struct zwp_virtual_keyboard_v1 *keyboard;
static struct zwlr_screencopy_frame_v1 *frame;
static struct wl_buffer *buffer;
static uint8_t *pixels;
static size_t allocation;
static uint32_t width, height, stride, format, flags, nonce;
static bool running, quit, buffer_busy, formats_done, offered_shm;
static uint64_t period_ns = 16000000, next_ns, request_ns, capture_serial;
static int gx = -1, gy, gw, gh, bottom_y;

static uint64_t now_ns(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) { perror("clock_gettime"); exit(2); }
    return (uint64_t)t.tv_sec * 1000000000u + (uint64_t)t.tv_nsec;
}
static void fail(const char *message) {
    fprintf(stderr, "frame-observer: %s (errno %d)\n", message, errno);
    exit(2);
}
static uint32_t checksum(uint32_t sequence) {
    uint32_t crc = UINT32_MAX;
    uint32_t words[2] = {sequence, nonce};
    for (size_t w = 0; w < 2; ++w) for (int shift = 24; shift >= 0; shift -= 8) {
        crc ^= (words[w] >> shift) & 255;
        for (int bit = 0; bit < 8; ++bit) crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1)));
    }
    return ~crc;
}
static uint32_t bytes_per_pixel(uint32_t f) {
    return f == WL_SHM_FORMAT_RGB888 || f == WL_SHM_FORMAT_BGR888 ? 3 : 4;
}
static uint32_t pixel(int x, int y) {
    if (x < 0 || y < 0 || (uint32_t)x >= width || (uint32_t)y >= height) return UINT32_MAX;
    if (flags & ZWLR_SCREENCOPY_FRAME_V1_FLAGS_Y_INVERT) y = (int)height - 1 - y;
    const uint8_t *at = pixels + (size_t)y * stride + (size_t)x * bytes_per_pixel(format);
    /* Three-byte formats, such as a GPU compositor's read format: BGR888
     * stores R, G, B in memory, RGB888 B, G, R. */
    if (format == WL_SHM_FORMAT_BGR888) return (uint32_t)at[0] << 16 | (uint32_t)at[1] << 8 | at[2];
    if (format == WL_SHM_FORMAT_RGB888) return (uint32_t)at[2] << 16 | (uint32_t)at[1] << 8 | at[0];
    uint32_t value;
    memcpy(&value, at, 4);
    if (format == WL_SHM_FORMAT_XRGB2101010 || format == WL_SHM_FORMAT_ARGB2101010 ||
        format == WL_SHM_FORMAT_XBGR2101010 || format == WL_SHM_FORMAT_ABGR2101010) {
        /* Ten-bit channels (a GPU compositor's read format) back to the
         * eight-bit colors they hold exactly. */
        uint32_t high = ((value >> 20 & 1023) * 255 + 511) / 1023, mid = ((value >> 10 & 1023) * 255 + 511) / 1023,
                 low = ((value & 1023) * 255 + 511) / 1023;
        if (format == WL_SHM_FORMAT_XRGB2101010 || format == WL_SHM_FORMAT_ARGB2101010)
            return high << 16 | mid << 8 | low;
        return low << 16 | mid << 8 | high;
    }
    if (format == WL_SHM_FORMAT_XBGR8888 || format == WL_SHM_FORMAT_ABGR8888)
        return ((value & 255) << 16) | (value & 0xff00) | ((value >> 16) & 255);
    return value & 0xffffff;
}
static bool color_cell(int x, int y, uint32_t color) {
    /* Five interior pixels reject partial cell updates and glyph contamination.
     * These are exact opaque colors, not a visual similarity threshold. */
    const int dx[5] = {gw / 2, gw / 4, 3 * gw / 4, gw / 4, 3 * gw / 4};
    const int dy[5] = {gh / 2, gh / 4, gh / 4, 3 * gh / 4, 3 * gh / 4};
    for (int at = 0; at < 5; ++at) if (pixel(x + dx[at], y + dy[at]) != color) return false;
    return true;
}
static bool marker(int x, int y, uint32_t *sequence) {
    if (gw < 2 || gh < 2 || (uint64_t)x + MARKER_CELLS * (uint64_t)gw > width ||
        (uint64_t)y + (uint64_t)gh > height) return false;
    for (int at = 0; at < 6; ++at) {
        if (!color_cell(x + at * gw, y, guard[at]) ||
            !color_cell(x + (70 + at) * gw, y, guard[5 - at])) return false;
    }
    uint64_t bits = 0;
    for (int at = 0; at < 64; ++at) {
        int cell = x + (6 + at) * gw;
        uint32_t value = pixel(cell + gw / 2, y + gh / 2);
        int bit = value == bit_color[0] ? 0 : value == bit_color[1] ? 1 : -1;
        if (bit < 0 || !color_cell(cell, y, bit_color[bit])) return false;
        bits = (bits << 1) | (unsigned)bit;
    }
    *sequence = (uint32_t)(bits >> 32);
    return (uint32_t)bits == checksum(*sequence);
}
static bool decode(uint32_t *sequence) {
    uint32_t other;
    if (gx >= 0) return marker(gx, gy, sequence) && marker(gx, bottom_y, &other) && *sequence == other;
    /* Only initial discovery scans the output. Normal observations inspect
     * 760 marker pixels. Explicit reset invalidates geometry after a resize. */
    for (uint32_t y = 0; y < height; ++y) for (uint32_t x = 0; x < width; ++x) {
        if (pixel((int)x, (int)y) != guard[0] || pixel((int)x - 1, (int)y) == guard[0] ||
            pixel((int)x, (int)y - 1) == guard[0]) continue;
        gw = 1; gh = 1;
        while (gw < 128 && pixel((int)x + gw, (int)y) == guard[0]) ++gw;
        while (gh < 256 && pixel((int)x, (int)y + gh) == guard[0]) ++gh;
        if (!marker((int)x, (int)y, sequence)) continue;
        for (int by = (int)y + 2 * gh; by + gh <= (int)height; by += gh) {
            if (marker((int)x, by, &other) && other == *sequence) {
                gx = (int)x; gy = (int)y; bottom_y = by;
                printf("{\"event\":\"geometry\",\"time_ns\":%" PRIu64 ",\"x\":%d,\"y\":%d,"
                       "\"cell_width\":%d,\"cell_height\":%d,\"rows\":%d,\"bottom_y\":%d}\n",
                       now_ns(), gx, gy, gw, gh, 1 + (by - gy) / gh, by);
                return true;
            }
        }
    }
    return false;
}
static void try_copy(void) {
    if (frame && formats_done && offered_shm && !buffer_busy) {
        buffer_busy = true;
        zwlr_screencopy_frame_v1_copy(frame, buffer);
    }
}
static void released(void *data, struct wl_buffer *object) {
    (void)data; (void)object;
    /* This buffer is never attached to a surface. Screencopy's ready event
     * completes its write; wl_buffer.release is not required by that protocol. */
}
static const struct wl_buffer_listener buffer_listener = {.release = released};
static void offered(void *data, struct zwlr_screencopy_frame_v1 *object, uint32_t f,
                    uint32_t w, uint32_t h, uint32_t s) {
    (void)data; (void)object;
    if (f != WL_SHM_FORMAT_XRGB8888 && f != WL_SHM_FORMAT_ARGB8888 &&
        f != WL_SHM_FORMAT_XBGR8888 && f != WL_SHM_FORMAT_ABGR8888 &&
        f != WL_SHM_FORMAT_XRGB2101010 && f != WL_SHM_FORMAT_ARGB2101010 &&
        f != WL_SHM_FORMAT_XBGR2101010 && f != WL_SHM_FORMAT_ABGR2101010 &&
        f != WL_SHM_FORMAT_RGB888 && f != WL_SHM_FORMAT_BGR888) {
        fprintf(stderr, "frame-observer: SHM format 0x%08x\n", f);
        fail("unsupported SHM format");
    }
    if (!w || !h || w > INT_MAX / 4 || h > INT_MAX || s < w * bytes_per_pixel(f) || s > INT_MAX ||
        (uint64_t)s * h > MAX_BUFFER) fail("invalid/oversized output dimensions");
    if (buffer && (w != width || h != height || s != stride || f != format)) fail("output geometry changed");
    if (!buffer) {
        width = w; height = h; stride = s; format = f; allocation = (size_t)s * h;
        int fd = memfd_create("hotaru-frame-observer", MFD_CLOEXEC);
        if (fd < 0 || ftruncate(fd, (off_t)allocation)) fail("SHM allocation");
        pixels = mmap(NULL, allocation, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        if (pixels == MAP_FAILED) fail("SHM mapping");
        struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, (int)allocation);
        buffer = wl_shm_pool_create_buffer(pool, 0, (int)w, (int)h, (int)s, f);
        wl_buffer_add_listener(buffer, &buffer_listener, NULL);
        wl_shm_pool_destroy(pool); close(fd);
    }
    offered_shm = true;
}
static void copy_flags(void *data, struct zwlr_screencopy_frame_v1 *object, uint32_t f) {
    (void)data; (void)object; flags = f;
}
static void ready(void *data, struct zwlr_screencopy_frame_v1 *object, uint32_t hi, uint32_t lo, uint32_t ns) {
    (void)data;
    uint64_t received = now_ns();
    uint32_t sequence = 0;
    bool valid = decode(&sequence);
    uint64_t decoded = now_ns();
    printf("{\"event\":\"capture\",\"capture\":%" PRIu64 ",\"request_ns\":%" PRIu64
           ",\"ready_ns\":%" PRIu64 ",\"decode_done_ns\":%" PRIu64 ",\"valid\":%s,\"sequence\":%u,"
           "\"compositor_sec_hi\":%u,\"compositor_sec_lo\":%u,\"compositor_nsec\":%u,\"flags\":%u}\n",
           capture_serial, request_ns, received, decoded, valid ? "true" : "false", sequence, hi, lo, ns, flags);
    zwlr_screencopy_frame_v1_destroy(object); frame = NULL; buffer_busy = false;
    next_ns = request_ns + period_ns;
}
static void failed(void *data, struct zwlr_screencopy_frame_v1 *object) {
    (void)data; zwlr_screencopy_frame_v1_destroy(object); frame = NULL;
    fail("screencopy failed");
}
static void damage(void *data, struct zwlr_screencopy_frame_v1 *object, uint32_t x, uint32_t y, uint32_t w, uint32_t h) {
    (void)data; (void)object; (void)x; (void)y; (void)w; (void)h;
}
static void dmabuf(void *data, struct zwlr_screencopy_frame_v1 *object, uint32_t f, uint32_t w, uint32_t h) {
    (void)data; (void)object; (void)f; (void)w; (void)h;
}
static void buffer_done(void *data, struct zwlr_screencopy_frame_v1 *object) {
    (void)data; (void)object; formats_done = true;
    if (!offered_shm) fail("SHM capture unsupported");
    try_copy();
}
static const struct zwlr_screencopy_frame_v1_listener copy_listener = {
    .buffer = offered, .flags = copy_flags, .ready = ready, .failed = failed,
    .damage = damage, .linux_dmabuf = dmabuf, .buffer_done = buffer_done
};
static void global(void *data, struct wl_registry *registry, uint32_t name, const char *interface, uint32_t version) {
    (void)data;
    if (!strcmp(interface, "wl_shm")) shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    else if (!strcmp(interface, "wl_output") && !output) output = wl_registry_bind(registry, name, &wl_output_interface, 1);
    else if (!strcmp(interface, "wl_seat") && !seat) seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    else if (!strcmp(interface, "zwlr_screencopy_manager_v1") && version >= 3)
        copy_manager = wl_registry_bind(registry, name, &zwlr_screencopy_manager_v1_interface, 3);
    else if (!strcmp(interface, "zwp_virtual_keyboard_manager_v1"))
        key_manager = wl_registry_bind(registry, name, &zwp_virtual_keyboard_manager_v1_interface, 1);
}
static void removed(void *data, struct wl_registry *registry, uint32_t name) { (void)data; (void)registry; (void)name; }
static const struct wl_registry_listener registry_listener = {.global = global, .global_remove = removed};

static void command(char *line) {
    uint32_t a, b;
    if (sscanf(line, "start %u %u", &a, &b) == 2) {
        if (b < 1000 || b > 1000000) fail("capture interval must be 1000..1000000 microseconds");
        nonce = a; period_ns = (uint64_t)b * 1000; running = true; next_ns = 0;
    } else if (!strcmp(line, "stop")) running = false;
    else if (!strcmp(line, "reset")) gx = -1;
    else if (!strcmp(line, "quit")) quit = true;
    else if (sscanf(line, "key %u %u", &a, &b) == 2) {
        if (b > 767) fail("invalid evdev keycode");
        uint64_t before = now_ns();
        uint32_t ms = (uint32_t)(before / 1000000);
        zwp_virtual_keyboard_v1_key(keyboard, ms, b, WL_KEYBOARD_KEY_STATE_PRESSED);
        zwp_virtual_keyboard_v1_key(keyboard, ms, b, WL_KEYBOARD_KEY_STATE_RELEASED);
        int rc = wl_display_flush(display);
        if (rc < 0 && errno != EAGAIN) fail("keyboard flush");
        printf("{\"event\":\"key\",\"sequence\":%u,\"inject_ns\":%" PRIu64
               ",\"flush_return_ns\":%" PRIu64 ",\"flush_complete\":%s}\n", a, before, now_ns(), rc >= 0 ? "true" : "false");
    } else fail("invalid observer command");
    printf("{\"event\":\"command\",\"time_ns\":%" PRIu64 ",\"running\":%s}\n", now_ns(), running ? "true" : "false");
}

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    display = wl_display_connect(NULL);
    if (!display) fail("Wayland connect");
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !shm || !output || !seat || !copy_manager || !key_manager)
        fail("required private compositor protocols unavailable");
    struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct xkb_rule_names names = {.layout = "us"};
    struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    char *text = keymap ? xkb_keymap_get_as_string(keymap, XKB_KEYMAP_FORMAT_TEXT_V1) : NULL;
    if (!text) fail("keymap creation");
    int fd = memfd_create("hotaru-frame-keymap", MFD_CLOEXEC);
    size_t length = strlen(text) + 1;
    if (fd < 0 || write(fd, text, length) != (ssize_t)length) fail("keymap allocation");
    keyboard = zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(key_manager, seat);
    zwp_virtual_keyboard_v1_keymap(keyboard, WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1, fd, (uint32_t)length);
    close(fd); free(text); xkb_keymap_unref(keymap); xkb_context_unref(context);
    if (wl_display_roundtrip(display) < 0) fail("keymap roundtrip");
    printf("{\"event\":\"ready\",\"time_ns\":%" PRIu64 "}\n", now_ns());
    int old_flags = fcntl(STDIN_FILENO, F_GETFL);
    if (old_flags < 0 || fcntl(STDIN_FILENO, F_SETFL, old_flags | O_NONBLOCK)) fail("stdin nonblocking");
    char input[4096]; size_t used = 0;
    while (!quit) {
        if (wl_display_dispatch_pending(display) < 0) fail("Wayland pending events");
        if (running && !frame && !buffer_busy && now_ns() >= next_ns) {
            formats_done = false; offered_shm = false; flags = 0;
            request_ns = now_ns(); ++capture_serial;
            frame = zwlr_screencopy_manager_v1_capture_output(copy_manager, 0, output);
            zwlr_screencopy_frame_v1_add_listener(frame, &copy_listener, NULL);
        }
        while (wl_display_prepare_read(display) != 0)
            if (wl_display_dispatch_pending(display) < 0) fail("Wayland prepare events");
        int flushed = wl_display_flush(display);
        if (flushed < 0 && errno != EAGAIN) { wl_display_cancel_read(display); fail("Wayland flush"); }
        struct pollfd fds[2] = {{wl_display_get_fd(display), POLLIN | (flushed < 0 ? POLLOUT : 0), 0}, {STDIN_FILENO, POLLIN, 0}};
        int timeout = -1;
        if (running && !frame && !buffer_busy) {
            uint64_t now = now_ns();
            timeout = next_ns > now ? (int)((next_ns - now + 999999) / 1000000) : 0;
        }
        int rc = poll(fds, 2, timeout);
        if (rc < 0) { wl_display_cancel_read(display); if (errno == EINTR) continue; fail("poll"); }
        if (fds[0].revents & POLLIN) { if (wl_display_read_events(display) < 0) fail("Wayland read"); }
        else wl_display_cancel_read(display);
        if (fds[0].revents & (POLLERR | POLLHUP)) fail("Wayland disconnected");
        if (fds[1].revents & (POLLIN | POLLHUP)) {
            ssize_t count = read(STDIN_FILENO, input + used, sizeof(input) - used - 1);
            if (!count) break;
            if (count < 0) { if (errno == EAGAIN || errno == EINTR) continue; fail("stdin read"); }
            used += (size_t)count; input[used] = 0;
            char *newline;
            while ((newline = memchr(input, '\n', used))) {
                size_t consumed = (size_t)(newline - input) + 1;
                *newline = 0; command(input);
                memmove(input, input + consumed, used - consumed); used -= consumed;
            }
            if (used == sizeof(input) - 1) fail("oversized command");
        }
    }
    if (frame) zwlr_screencopy_frame_v1_destroy(frame);
    if (buffer) wl_buffer_destroy(buffer);
    if (pixels) munmap(pixels, allocation);
    zwp_virtual_keyboard_v1_destroy(keyboard);
    wl_display_disconnect(display);
    return 0;
}
