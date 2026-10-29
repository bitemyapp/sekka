/* Private-compositor pointer fixture; never connect to a live desktop. */
#define _DEFAULT_SOURCE
#include <linux/input-event-codes.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>
#include "virtual-pointer-client.h"

static struct wl_seat *seat;
static struct zwlr_virtual_pointer_manager_v1 *manager;
static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    (void)data; (void)version;
    if (!strcmp(interface, "wl_seat"))
        seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    if (!strcmp(interface, "zwlr_virtual_pointer_manager_v1"))
        manager = wl_registry_bind(registry, name,
                                   &zwlr_virtual_pointer_manager_v1_interface, 1);
}
static void removed(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}
static const struct wl_registry_listener listener = { global, removed };

/* pointer X Y WIDTH HEIGHT [left|middle|right [CLICKS]] */
int main(int argc, char **argv) {
    if (argc < 5 || argc > 7) return 2;
    uint32_t button = BTN_LEFT;
    if (argc > 5 && !strcmp(argv[5], "middle")) button = BTN_MIDDLE;
    else if (argc > 5 && !strcmp(argv[5], "right")) button = BTN_RIGHT;
    else if (argc > 5 && strcmp(argv[5], "left")) return 2;
    int clicks = argc > 6 ? atoi(argv[6]) : 1;
    if (clicks < 1 || clicks > 3) return 2;
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 3;
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !seat || !manager) return 4;
    struct zwlr_virtual_pointer_v1 *pointer =
        zwlr_virtual_pointer_manager_v1_create_virtual_pointer(manager, seat);
    wl_display_roundtrip(display);
    /* Let the client bind wl_pointer on a previously pointer-less seat. */
    usleep(150000);
    zwlr_virtual_pointer_v1_motion_absolute(pointer, 1,
        (uint32_t)strtoul(argv[1], NULL, 10), (uint32_t)strtoul(argv[2], NULL, 10),
        (uint32_t)strtoul(argv[3], NULL, 10), (uint32_t)strtoul(argv[4], NULL, 10));
    zwlr_virtual_pointer_v1_frame(pointer);
    wl_display_roundtrip(display);
    /* Clicks in quick succession, as a double or triple click. */
    for (int i = 0; i < clicks; i++) {
        zwlr_virtual_pointer_v1_button(pointer, 2 + 2 * i, button, WL_POINTER_BUTTON_STATE_PRESSED);
        zwlr_virtual_pointer_v1_frame(pointer);
        wl_display_roundtrip(display);
        zwlr_virtual_pointer_v1_button(pointer, 3 + 2 * i, button, WL_POINTER_BUTTON_STATE_RELEASED);
        zwlr_virtual_pointer_v1_frame(pointer);
        wl_display_roundtrip(display);
    }
    zwlr_virtual_pointer_v1_destroy(pointer);
    zwlr_virtual_pointer_manager_v1_destroy(manager);
    wl_seat_destroy(seat);
    wl_registry_destroy(registry);
    wl_display_disconnect(display);
    return 0;
}
