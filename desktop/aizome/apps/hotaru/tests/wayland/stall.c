/* Private-compositor clipboard owner that never sends its data; never
 * connect to a live desktop. `stall [primary]` offers text as the clipboard
 * (or the primary selection) and keeps every requested pipe open, unwritten,
 * until it is killed or replaced. */
#define _DEFAULT_SOURCE
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>
#include "data-control-client.h"

static struct wl_seat *seat;
static struct zwlr_data_control_manager_v1 *manager;
static int held[256], holding;

static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    (void)data; (void)version;
    if (!strcmp(interface, "wl_seat"))
        seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    if (!strcmp(interface, "zwlr_data_control_manager_v1"))
        manager = wl_registry_bind(registry, name, &zwlr_data_control_manager_v1_interface, 2);
}
static void removed(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}
static const struct wl_registry_listener registry_listener = { global, removed };

static void send(void *data, struct zwlr_data_control_source_v1 *source, const char *mime, int32_t fd) {
    (void)data; (void)source; (void)mime;
    if (holding < 256) held[holding++] = fd; else close(fd);
}
static void cancelled(void *data, struct zwlr_data_control_source_v1 *source) {
    (void)data; (void)source;
    exit(0);
}
static const struct zwlr_data_control_source_v1_listener source_listener = { send, cancelled };

static void offer(void *data, struct zwlr_data_control_device_v1 *device, struct zwlr_data_control_offer_v1 *id) {
    (void)data; (void)device; (void)id;
}
static void selection(void *data, struct zwlr_data_control_device_v1 *device, struct zwlr_data_control_offer_v1 *id) {
    (void)data; (void)device; (void)id;
}
static void finished(void *data, struct zwlr_data_control_device_v1 *device) {
    (void)data; (void)device;
    exit(0);
}
static const struct zwlr_data_control_device_v1_listener device_listener = { offer, selection, finished, selection };

int main(int argc, char **argv) {
    int primary = argc > 1 && !strcmp(argv[1], "primary");
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 3;
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !seat || !manager) return 4;
    struct zwlr_data_control_device_v1 *device = zwlr_data_control_manager_v1_get_data_device(manager, seat);
    zwlr_data_control_device_v1_add_listener(device, &device_listener, NULL);
    struct zwlr_data_control_source_v1 *source = zwlr_data_control_manager_v1_create_data_source(manager);
    zwlr_data_control_source_v1_add_listener(source, &source_listener, NULL);
    const char *types[] = { "text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "TEXT", "STRING" };
    for (size_t i = 0; i < sizeof types / sizeof *types; i++)
        zwlr_data_control_source_v1_offer(source, types[i]);
    if (primary)
        zwlr_data_control_device_v1_set_primary_selection(device, source);
    else
        zwlr_data_control_device_v1_set_selection(device, source);
    while (wl_display_dispatch(display) >= 0) {}
    return 0;
}
