/* Combined keyboard/pointer source for a private compositor only. */
#define _GNU_SOURCE
#include <linux/input-event-codes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include "virtual-pointer-client.h"
#include "virtual-keyboard-client.h"
static struct wl_seat *seat;
static struct zwlr_virtual_pointer_manager_v1 *pm;
static struct zwp_virtual_keyboard_manager_v1 *km;
static void global(void *data, struct wl_registry *r, uint32_t name, const char *interface, uint32_t version) {
    (void)data; (void)version;
    if (!strcmp(interface, "wl_seat")) seat = wl_registry_bind(r, name, &wl_seat_interface, 1);
    if (!strcmp(interface, "zwlr_virtual_pointer_manager_v1")) pm = wl_registry_bind(r, name, &zwlr_virtual_pointer_manager_v1_interface, 1);
    if (!strcmp(interface, "zwp_virtual_keyboard_manager_v1")) km = wl_registry_bind(r, name, &zwp_virtual_keyboard_manager_v1_interface, 1);
}
static void removed(void *data, struct wl_registry *r, uint32_t name) { (void)data; (void)r; (void)name; }
static const struct wl_registry_listener listener = {global, removed};
int main(int argc, char **argv) {
    if (argc != 4) return 2;
    struct wl_display *d = wl_display_connect(NULL);
    if (!d) return 3;
    struct wl_registry *r = wl_display_get_registry(d);
    wl_registry_add_listener(r, &listener, NULL);
    if (wl_display_roundtrip(d) < 0 || !seat || !pm || !km) return 4;
    struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct xkb_rule_names names = {.layout = "us"};
    struct xkb_keymap *map = xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    char *text = xkb_keymap_get_as_string(map, XKB_KEYMAP_FORMAT_TEXT_V1);
    int fd = memfd_create("hotaru-link-keymap", MFD_CLOEXEC);
    size_t length = strlen(text) + 1;
    if (fd < 0 || write(fd, text, length) != (ssize_t)length) return 5;
    struct zwp_virtual_keyboard_v1 *k = zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(km, seat);
    struct zwlr_virtual_pointer_v1 *p = zwlr_virtual_pointer_manager_v1_create_virtual_pointer(pm, seat);
    zwp_virtual_keyboard_v1_keymap(k, WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1, fd, (uint32_t)length);
    close(fd); wl_display_roundtrip(d); usleep(150000);
    int super = strcmp(argv[3], "plain") != 0;
    if (super) {
        zwp_virtual_keyboard_v1_key(k, 1, KEY_LEFTMETA, WL_KEYBOARD_KEY_STATE_PRESSED);
        zwp_virtual_keyboard_v1_modifiers(k, 1u << xkb_keymap_mod_get_index(map, XKB_MOD_NAME_LOGO), 0, 0, 0);
    }
    uint32_t x = (uint32_t)strtoul(argv[1], NULL, 10), y = (uint32_t)strtoul(argv[2], NULL, 10);
    zwlr_virtual_pointer_v1_motion_absolute(p, 2, x, y, 1280, 800);
    zwlr_virtual_pointer_v1_frame(p); wl_display_roundtrip(d);
    if (!strcmp(argv[3], "sequence")) {
        puts("ready"); fflush(stdout);
        int command;
        while ((command = getchar()) != EOF && command != 'q') {
            if (command == '\n') continue;
            if (command == 'c' || command == 'd') {
                zwlr_virtual_pointer_v1_button(p, 3, BTN_LEFT, WL_POINTER_BUTTON_STATE_PRESSED);
                zwlr_virtual_pointer_v1_frame(p); wl_display_roundtrip(d);
            }
            if (command == 'c' || command == 'u') {
                zwlr_virtual_pointer_v1_button(p, 5, BTN_LEFT, WL_POINTER_BUTTON_STATE_RELEASED);
                zwlr_virtual_pointer_v1_frame(p); wl_display_roundtrip(d);
            }
            if (command == 'm') {
                zwlr_virtual_pointer_v1_motion_absolute(p, 4, x + 1, y, 1280, 800);
                zwlr_virtual_pointer_v1_frame(p); wl_display_roundtrip(d);
            }
            if (command == 'f') {
                zwp_virtual_keyboard_v1_key(k, 8, KEY_F, WL_KEYBOARD_KEY_STATE_PRESSED);
                zwp_virtual_keyboard_v1_key(k, 9, KEY_F, WL_KEYBOARD_KEY_STATE_RELEASED);
                wl_display_roundtrip(d);
            }
            puts("ack"); fflush(stdout);
        }
        goto cleanup;
    }
    int hover = !strcmp(argv[3], "hover");
    if (!hover) {
        zwlr_virtual_pointer_v1_button(p, 3, BTN_LEFT, WL_POINTER_BUTTON_STATE_PRESSED);
        zwlr_virtual_pointer_v1_frame(p); wl_display_roundtrip(d);
    }
    puts("ready"); fflush(stdout);
    if (hover || !strcmp(argv[3], "hold")) (void)getchar();
    if (!strcmp(argv[3], "drag")) {
        zwlr_virtual_pointer_v1_motion_absolute(p, 4, x + 1, y, 1280, 800);
        zwlr_virtual_pointer_v1_frame(p); wl_display_roundtrip(d);
    }
    if (!hover) {
        zwlr_virtual_pointer_v1_button(p, 5, BTN_LEFT, WL_POINTER_BUTTON_STATE_RELEASED);
        zwlr_virtual_pointer_v1_frame(p); wl_display_roundtrip(d);
    }
    /* Keep the device and pointer stationary while Python checks the visible
     * opener error. Destroying it first would hide hover-priority bugs. */
    if (!strcmp(argv[3], "stationary")) (void)getchar();
cleanup:
    if (super) {
        zwp_virtual_keyboard_v1_modifiers(k, 0, 0, 0, 0);
        zwp_virtual_keyboard_v1_key(k, 6, KEY_LEFTMETA, WL_KEYBOARD_KEY_STATE_RELEASED);
        wl_display_roundtrip(d);
    }
    zwlr_virtual_pointer_v1_destroy(p); zwp_virtual_keyboard_v1_destroy(k);
    zwlr_virtual_pointer_manager_v1_destroy(pm); zwp_virtual_keyboard_manager_v1_destroy(km);
    wl_seat_destroy(seat); wl_registry_destroy(r); wl_display_disconnect(d);
    free(text); xkb_keymap_unref(map); xkb_context_unref(context);
    return 0;
}
