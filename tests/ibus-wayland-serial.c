/* Exercise the real IBus registry callback. Protocol calls are inert: this
 * checks serial bookkeeping without a compositor, input engine or display. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct wl_registry { int unused; };
struct interface { const char *name; };
#define INTERFACE(name) static const struct interface name##_interface = { #name }
INTERFACE(zwp_input_method_manager_v2);
INTERFACE(zwp_input_method_v1);
INTERFACE(zwp_input_panel_v1);
INTERFACE(wl_seat);
INTERFACE(zwp_virtual_keyboard_manager_v1);
enum { INPUT_METHOD_V1, INPUT_METHOD_V2 };
typedef struct {
    void *seat;
    uint32_t wl_name;
    void *input_method_v2;
} IBusWaylandSeat;
typedef struct {
    int verbose;
    FILE *log;
    uint32_t im_serial;
    int version;
    void *input_method_manager_v2, *input_method_v1, *panel, *seats;
    IBusWaylandSeat *seat;
} IBusWaylandIMPrivate;
typedef struct { IBusWaylandIMPrivate priv; } IBusWaylandIM;
static void *_virtual_keyboard_manager;
static const int input_method_listener_v1, input_method_listener_v2, seat_listener;
#define IBUS_IS_WAYLAND_IM(wlim) ((wlim) != NULL)
#define g_return_if_fail(condition) assert(condition)
#define g_strcmp0(a, b) strcmp(a, b)
#define g_slice_new0(type) calloc(1, sizeof(type))
#define ibus_wayland_im_get_instance_private(wlim) (&(wlim)->priv)

static void *wl_registry_bind(struct wl_registry *r, uint32_t n,
                             const struct interface *i, uint32_t v)
{
    (void)r; (void)n; (void)v;
    return (void *)i;
}
static void listener(void *object, const void *callbacks, void *data)
{
    (void)object; (void)callbacks; (void)data;
}
#define wl_seat_add_listener listener
#define zwp_input_method_v1_add_listener listener
#define zwp_input_method_v2_add_listener listener
static void g_ptr_array_add(void *array, void *value) { (void)array; (void)value; }
static void *zwp_input_method_manager_v2_get_input_method(void *manager, void *seat)
{
    (void)manager;
    return seat;
}

#include "registry-handler.inc"

int main(void)
{
    IBusWaylandIM im = {0};
    struct wl_registry registry = {0};
    const char *unrelated[] = {
        "wl_output", "zxdg_output_manager_v1", "wl_output",
        "zwp_input_panel_v1", "zwp_virtual_keyboard_manager_v1", "wl_output"
    };
    const uint32_t serials[] = {1, 37, UINT32_MAX};
    for (size_t n = 0; n < sizeof(serials) / sizeof(serials[0]); ++n) {
        im.priv.im_serial = serials[n];
        for (size_t i = 0; i < sizeof(unrelated) / sizeof(unrelated[0]); ++i) {
            registry_handle_global(&im, &registry, 100 + i, unrelated[i], 1);
            if (im.priv.im_serial != serials[n]) {
                fprintf(stderr, "%s reset serial %u to %u\n", unrelated[i],
                        serials[n], im.priv.im_serial);
                return 42; /* The precise original regression, not a build failure. */
            }
        }
    }
    registry_handle_global(&im, &registry, 200, "zwp_input_method_manager_v2", 1);
    assert(im.priv.im_serial == 0 && im.priv.version == INPUT_METHOD_V2);
    im.priv.im_serial = 37;
    registry_handle_global(&im, &registry, 201, "zwp_input_method_v1", 1);
    assert(im.priv.im_serial == 0 && im.priv.version == INPUT_METHOD_V1);
    puts("IBus preserves serials across repeated output globals; new input methods reset them");
    return 0;
}
