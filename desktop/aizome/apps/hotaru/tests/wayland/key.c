/* A private-compositor test input source. Build using the generated header and
 * protocol from virtual-keyboard-unstable-v1.xml; never point at a live session. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/input-event-codes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include "virtual-keyboard-client.h"

static struct wl_seat *seat;
static struct zwp_virtual_keyboard_manager_v1 *manager;

static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    (void)data; (void)version;
    if (!strcmp(interface, "wl_seat"))
        seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    if (!strcmp(interface, "zwp_virtual_keyboard_manager_v1"))
        manager = wl_registry_bind(registry, name,
                                  &zwp_virtual_keyboard_manager_v1_interface, 1);
}
static void removed(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}
static const struct wl_registry_listener listener = { global, removed };

int main(int argc, char **argv) {
    if (argc < 3 || argc > 129 || argc % 2 != 1) {
        fprintf(stderr, "usage: key EVDEV_CODE MODIFIERS [EVDEV_CODE MODIFIERS ...]\n"); return 2;
    }
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 3;
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !seat || !manager) return 4;
    struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct xkb_rule_names names = { .layout = getenv("HOTARU_TEST_LAYOUT") };
    struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    char *text = xkb_keymap_get_as_string(keymap, XKB_KEYMAP_FORMAT_TEXT_V1);
    if (!text) return 5;
    int fd = memfd_create("hotaru-test-keymap", MFD_CLOEXEC);
    size_t length = strlen(text) + 1;
    if (fd < 0 || write(fd, text, length) != (ssize_t)length) return 6;
    struct zwp_virtual_keyboard_v1 *keyboard = zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(manager, seat);
    zwp_virtual_keyboard_v1_keymap(keyboard, WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1, fd, (uint32_t)length);
    close(fd);
    wl_display_roundtrip(display);
    /* A previously keyboard-less headless seat must let the app bind wl_keyboard
     * and receive focus before the first injected key is sent. */
    usleep(150000);
    wl_display_roundtrip(display);
    for (int at = 1; at < argc; at += 2) {
        uint32_t modifiers = 0;
        if (strstr(argv[at + 1], "super")) modifiers |= 1u << xkb_keymap_mod_get_index(keymap, XKB_MOD_NAME_LOGO);
        if (strstr(argv[at + 1], "shift")) modifiers |= 1u << xkb_keymap_mod_get_index(keymap, XKB_MOD_NAME_SHIFT);
        if (strstr(argv[at + 1], "ctrl")) modifiers |= 1u << xkb_keymap_mod_get_index(keymap, XKB_MOD_NAME_CTRL);
        if (strstr(argv[at + 1], "alt")) modifiers |= 1u << xkb_keymap_mod_get_index(keymap, XKB_MOD_NAME_ALT);
        zwp_virtual_keyboard_v1_modifiers(keyboard, modifiers, 0, 0, 0);
        zwp_virtual_keyboard_v1_key(keyboard, (uint32_t)at, (uint32_t)strtoul(argv[at], NULL, 10), WL_KEYBOARD_KEY_STATE_PRESSED);
        zwp_virtual_keyboard_v1_key(keyboard, (uint32_t)at + 1, (uint32_t)strtoul(argv[at], NULL, 10), WL_KEYBOARD_KEY_STATE_RELEASED);
        zwp_virtual_keyboard_v1_modifiers(keyboard, 0, 0, 0, 0);
    }
    wl_display_roundtrip(display);
    zwp_virtual_keyboard_v1_destroy(keyboard);
    zwp_virtual_keyboard_manager_v1_destroy(manager);
    wl_seat_destroy(seat);
    wl_registry_destroy(registry);
    wl_display_disconnect(display);
    free(text); xkb_keymap_unref(keymap); xkb_context_unref(context);
    return 0;
}
