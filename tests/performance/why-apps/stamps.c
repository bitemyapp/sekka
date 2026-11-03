/* stamps.so: time-stamp a program's start-up at the boundaries between its
 * libraries, by interposition (LD_PRELOAD), cheaply enough that the
 * program's own timing barely moves.
 *
 *   STAMPS_FILE=/path/out.txt LD_PRELOAD=/path/stamps.so PROGRAM ...
 *
 * Each line is "<CLOCK_MONOTONIC ns> <pid> <tid> <event> <duration ns>
 * <detail>", appended with one write(2), so several processes and threads
 * can share the file. What is stamped:
 *
 * - ctor: this library's constructor, which runs after the dynamic linker
 *   has mapped and relocated every DT_NEEDED library (glibc relocates them
 *   all before running any initialiser); main: the program's main();
 * - dlopen of a path (the libraries loaded at run time: GPU drivers, Vulkan
 *   ICDs, Qt plugins, GIO modules), with its duration. A bare name is
 *   passed on by a tail call, unstamped, so that the caller's own run path
 *   still decides where it is found;
 * - EGL, Vulkan, fontconfig, xkbcommon, GIO D-Bus and libdbus calls that
 *   start a subsystem or block on a reply, with durations, whether they are
 *   called through the PLT or looked up with dlsym (libepoxy, the Vulkan
 *   loader's users, fontique, QtDBus);
 * - Wayland: wl_display_connect, roundtrips, the xdg toplevel's creation and
 *   every wl_surface attach and commit (commit-buffer marks a commit after a
 *   non-null attach: a frame);
 * - poll/ppoll calls that block for 200 us or more (time spent waiting);
 * - pthread_create, with the symbol of the start routine.
 *
 * STAMPS_EXIT=1 makes the process _exit(0) right after its first commit of
 * a buffer to an xdg toplevel's surface is flushed, so that counters taken
 * at exit (perf stat, strace -c) cover exactly launch to first frame.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <link.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static int out_fd = -1;
static int exit_at_frame;
static void *(*real_dlsym)(void *, const char *);

static inline uint64_t now_ns(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static void emit(uint64_t t, const char *ev, uint64_t dur, const char *fmt, ...) {
  if (out_fd < 0) return;
  char buf[768];
  int n = snprintf(buf, sizeof buf, "%llu %d %ld %s %llu ", (unsigned long long)t, getpid(),
                   (long)syscall(SYS_gettid), ev, (unsigned long long)dur);
  if (n < 0 || n >= (int)sizeof buf) return;
  if (fmt) {
    va_list ap;
    va_start(ap, fmt);
    int m = vsnprintf(buf + n, sizeof buf - n - 1, fmt, ap);
    va_end(ap);
    if (m > 0) n += m < (int)(sizeof buf - n - 1) ? m : (int)(sizeof buf - n - 2);
  }
  buf[n++] = '\n';
  ssize_t w = write(out_fd, buf, n);
  (void)w;
}

/* The loaded objects' names, gathered first: dlopen isn't called while
 * dl_iterate_phdr holds the loader's lock. */
struct names {
  char list[256][256];
  int n;
};
static int collect(struct dl_phdr_info *info, size_t size, void *data) {
  (void)size;
  struct names *ns = data;
  if (ns->n < 256 && info->dlpi_name && info->dlpi_name[0] && !strstr(info->dlpi_name, "stamps.so"))
    snprintf(ns->list[ns->n++], 256, "%s", info->dlpi_name);
  return 0;
}

static void *find_real(const char *name) {
  if (!real_dlsym) real_dlsym = (void *(*)(void *, const char *))dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.34");
  if (!real_dlsym) real_dlsym = (void *(*)(void *, const char *))dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.2.5");
  void *r = real_dlsym(RTLD_NEXT, name);
  if (r) return r;
  /* A library dlopened with RTLD_LOCAL (libwayland-client under Qt's
   * platform plugin) isn't in the scope RTLD_NEXT searches: look in each
   * loaded object by its own handle. */
  static struct names ns;
  ns.n = 0;
  dl_iterate_phdr(collect, &ns);
  void *(*open_)(const char *, int) = (void *(*)(const char *, int))real_dlsym(RTLD_NEXT, "dlopen");
  for (int i = 0; i < ns.n && !r; i++) {
    void *h = open_(ns.list[i], RTLD_NOLOAD | RTLD_LAZY);
    if (!h) continue;
    r = real_dlsym(h, name);
    dlclose(h);
  }
  return r;
}

__attribute__((constructor)) static void stamps_init(void) {
  uint64_t t = now_ns();
  const char *f = getenv("STAMPS_FILE");
  if (f && *f) out_fd = open(f, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
  exit_at_frame = getenv("STAMPS_EXIT") && getenv("STAMPS_EXIT")[0] == '1';
  char exe[512];
  ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
  exe[n > 0 ? n : 0] = 0;
  emit(t, "ctor", 0, "%s", exe);
}

/* main() */
static int (*app_main)(int, char **, char **);
static int main_hook(int argc, char **argv, char **envp) {
  emit(now_ns(), "main", 0, "%s", argv[0]);
  return app_main(argc, argv, envp);
}
int __libc_start_main(int (*main)(int, char **, char **), int argc, char **argv, void (*init)(void),
                      void (*fini)(void), void (*rtld_fini)(void), void *stack_end) {
  int (*real)(int (*)(int, char **, char **), int, char **, void (*)(void), void (*)(void), void (*)(void), void *) =
      find_real("__libc_start_main");
  app_main = main;
  return real(main_hook, argc, argv, init, fini, rtld_fini, stack_end);
}

/* dlopen of a path, timed; of a bare name, a tail call. */
static void *(*real_dlopen)(const char *, int);
void *dlopen(const char *name, int flags) {
  if (!real_dlopen) real_dlopen = find_real("dlopen");
  if (!name || !strchr(name, '/')) __attribute__((musttail)) return real_dlopen(name, flags);
  uint64_t t = now_ns();
  void *h = real_dlopen(name, flags);
  emit(t, "dlopen", now_ns() - t, "%s%s", name, h ? "" : " FAILED");
  return h;
}

/* Interposed calls: each has a wrapper, the real function (found through
 * RTLD_NEXT, or kept from the dlsym that asked for it) and an entry in
 * the table dlsym consults. */
struct hook {
  const char *name;
  void *wrapper;
  void **real;
};
#define REAL(fn) static __typeof__(fn) *real_##fn
#define GET(fn) ({ if (!real_##fn) real_##fn = find_real(#fn); real_##fn; })

/* A timed call: T(event, detail format, args...) around expr. */
#define TIMED(ret, call, ev, ...)       \
  do {                                  \
    uint64_t t_ = now_ns();             \
    ret = call;                         \
    emit(t_, ev, now_ns() - t_, __VA_ARGS__); \
  } while (0)

/* EGL */
typedef void *EGLDisplay;
typedef unsigned EGLBoolean;
EGLDisplay eglGetDisplay(void *native);
EGLDisplay eglGetPlatformDisplay(unsigned platform, void *native, const intptr_t *attrs);
EGLDisplay eglGetPlatformDisplayEXT(unsigned platform, void *native, const int *attrs);
EGLBoolean eglInitialize(EGLDisplay d, int *major, int *minor);
void *eglCreateContext(EGLDisplay d, void *config, void *share, const int *attrs);
EGLBoolean eglMakeCurrent(EGLDisplay d, void *draw, void *read, void *ctx);
REAL(eglGetDisplay);
REAL(eglGetPlatformDisplay);
REAL(eglGetPlatformDisplayEXT);
REAL(eglInitialize);
REAL(eglCreateContext);
REAL(eglMakeCurrent);
EGLDisplay eglGetDisplay(void *native) {
  EGLDisplay r;
  TIMED(r, GET(eglGetDisplay)(native), "eglGetDisplay", "");
  return r;
}
EGLDisplay eglGetPlatformDisplay(unsigned platform, void *native, const intptr_t *attrs) {
  EGLDisplay r;
  TIMED(r, GET(eglGetPlatformDisplay)(platform, native, attrs), "eglGetPlatformDisplay", "0x%x", platform);
  return r;
}
EGLDisplay eglGetPlatformDisplayEXT(unsigned platform, void *native, const int *attrs) {
  EGLDisplay r;
  TIMED(r, GET(eglGetPlatformDisplayEXT)(platform, native, attrs), "eglGetPlatformDisplayEXT", "0x%x", platform);
  return r;
}
EGLBoolean eglInitialize(EGLDisplay d, int *major, int *minor) {
  EGLBoolean r;
  TIMED(r, GET(eglInitialize)(d, major, minor), "eglInitialize", "%u", r);
  return r;
}
void *eglCreateContext(EGLDisplay d, void *config, void *share, const int *attrs) {
  void *r;
  TIMED(r, GET(eglCreateContext)(d, config, share, attrs), "eglCreateContext", "%s", r ? "ok" : "FAILED");
  return r;
}
static int made_current;
EGLBoolean eglMakeCurrent(EGLDisplay d, void *draw, void *read, void *ctx) {
  EGLBoolean r;
  if (made_current++ < 8) TIMED(r, GET(eglMakeCurrent)(d, draw, read, ctx), "eglMakeCurrent", "");
  else r = GET(eglMakeCurrent)(d, draw, read, ctx);
  return r;
}

/* Vulkan */
typedef int VkResult;
VkResult vkCreateInstance(const void *info, const void *alloc, void **instance);
VkResult vkEnumeratePhysicalDevices(void *instance, uint32_t *count, void **devices);
VkResult vkCreateDevice(void *phys, const void *info, const void *alloc, void **device);
VkResult vkCreateGraphicsPipelines(void *device, uint64_t cache, uint32_t n, const void *infos, const void *alloc,
                                   uint64_t *pipelines);
VkResult vkEnumerateInstanceExtensionProperties(const char *layer, uint32_t *count, void *props);
REAL(vkCreateInstance);
REAL(vkEnumeratePhysicalDevices);
REAL(vkCreateDevice);
REAL(vkCreateGraphicsPipelines);
REAL(vkEnumerateInstanceExtensionProperties);
VkResult vkCreateInstance(const void *info, const void *alloc, void **instance) {
  VkResult r;
  TIMED(r, GET(vkCreateInstance)(info, alloc, instance), "vkCreateInstance", "%d", r);
  return r;
}
VkResult vkEnumeratePhysicalDevices(void *instance, uint32_t *count, void **devices) {
  VkResult r;
  TIMED(r, GET(vkEnumeratePhysicalDevices)(instance, count, devices), "vkEnumeratePhysicalDevices", "%u",
        count ? *count : 0);
  return r;
}
VkResult vkCreateDevice(void *phys, const void *info, const void *alloc, void **device) {
  VkResult r;
  TIMED(r, GET(vkCreateDevice)(phys, info, alloc, device), "vkCreateDevice", "%d", r);
  return r;
}
VkResult vkCreateGraphicsPipelines(void *device, uint64_t cache, uint32_t n, const void *infos, const void *alloc,
                                   uint64_t *pipelines) {
  VkResult r;
  TIMED(r, GET(vkCreateGraphicsPipelines)(device, cache, n, infos, alloc, pipelines), "vkCreateGraphicsPipelines",
        "%u", n);
  return r;
}
VkResult vkEnumerateInstanceExtensionProperties(const char *layer, uint32_t *count, void *props) {
  VkResult r;
  TIMED(r, GET(vkEnumerateInstanceExtensionProperties)(layer, count, props),
        "vkEnumerateInstanceExtensionProperties", "");
  return r;
}

/* fontconfig */
typedef void FcConfig;
typedef void FcPattern;
typedef void FcObjectSet;
typedef void FcFontSet;
int FcInit(void);
FcConfig *FcInitLoadConfigAndFonts(void);
FcConfig *FcConfigGetCurrent(void);
FcFontSet *FcFontList(FcConfig *c, FcPattern *p, FcObjectSet *os);
FcFontSet *FcFontSort(FcConfig *c, FcPattern *p, int trim, void **csp, int *result);
FcPattern *FcFontMatch(FcConfig *c, FcPattern *p, int *result);
REAL(FcInit);
REAL(FcInitLoadConfigAndFonts);
REAL(FcConfigGetCurrent);
REAL(FcFontList);
REAL(FcFontSort);
REAL(FcFontMatch);
int FcInit(void) {
  int r;
  TIMED(r, GET(FcInit)(), "FcInit", "");
  return r;
}
FcConfig *FcInitLoadConfigAndFonts(void) {
  FcConfig *r;
  TIMED(r, GET(FcInitLoadConfigAndFonts)(), "FcInitLoadConfigAndFonts", "");
  return r;
}
FcConfig *FcConfigGetCurrent(void) {
  uint64_t t = now_ns();
  FcConfig *r = GET(FcConfigGetCurrent)();
  uint64_t d = now_ns() - t;
  if (d > 100000) emit(t, "FcConfigGetCurrent", d, "");
  return r;
}
FcFontSet *FcFontList(FcConfig *c, FcPattern *p, FcObjectSet *os) {
  FcFontSet *r;
  TIMED(r, GET(FcFontList)(c, p, os), "FcFontList", "%d", r ? *(int *)r : -1);
  return r;
}
FcFontSet *FcFontSort(FcConfig *c, FcPattern *p, int trim, void **csp, int *result) {
  FcFontSet *r;
  TIMED(r, GET(FcFontSort)(c, p, trim, csp, result), "FcFontSort", "%d", r ? *(int *)r : -1);
  return r;
}
FcPattern *FcFontMatch(FcConfig *c, FcPattern *p, int *result) {
  FcPattern *r;
  TIMED(r, GET(FcFontMatch)(c, p, result), "FcFontMatch", "");
  return r;
}

/* xkbcommon */
void *xkb_context_new(int flags);
void *xkb_keymap_new_from_string(void *ctx, const char *s, int format, int flags);
void *xkb_keymap_new_from_buffer(void *ctx, const char *s, size_t len, int format, int flags);
void *xkb_compose_table_new_from_locale(void *ctx, const char *locale, int flags);
REAL(xkb_context_new);
REAL(xkb_keymap_new_from_string);
REAL(xkb_keymap_new_from_buffer);
REAL(xkb_compose_table_new_from_locale);
void *xkb_context_new(int flags) {
  void *r;
  TIMED(r, GET(xkb_context_new)(flags), "xkb_context_new", "");
  return r;
}
void *xkb_keymap_new_from_string(void *ctx, const char *s, int format, int flags) {
  void *r;
  TIMED(r, GET(xkb_keymap_new_from_string)(ctx, s, format, flags), "xkb_keymap_new", "");
  return r;
}
void *xkb_keymap_new_from_buffer(void *ctx, const char *s, size_t len, int format, int flags) {
  void *r;
  TIMED(r, GET(xkb_keymap_new_from_buffer)(ctx, s, len, format, flags), "xkb_keymap_new", "%zu", len);
  return r;
}
void *xkb_compose_table_new_from_locale(void *ctx, const char *locale, int flags) {
  void *r;
  TIMED(r, GET(xkb_compose_table_new_from_locale)(ctx, locale, flags), "xkb_compose_table", "%s", locale);
  return r;
}

/* GIO D-Bus (blocking calls) */
typedef void GDBusConnection;
typedef void GVariant;
GDBusConnection *g_bus_get_sync(int type, void *cancellable, void **error);
GDBusConnection *g_dbus_connection_new_for_address_sync(const char *address, int flags, void *observer,
                                                        void *cancellable, void **error);
GVariant *g_dbus_connection_call_sync(GDBusConnection *c, const char *bus, const char *path, const char *iface,
                                      const char *method, GVariant *params, const void *reply_type, int flags,
                                      int timeout, void *cancellable, void **error);
void *g_dbus_proxy_new_sync(GDBusConnection *c, int flags, void *info, const char *name, const char *path,
                            const char *iface, void *cancellable, void **error);
void *g_dbus_proxy_new_for_bus_sync(int type, int flags, void *info, const char *name, const char *path,
                                    const char *iface, void *cancellable, void **error);
REAL(g_bus_get_sync);
REAL(g_dbus_connection_new_for_address_sync);
REAL(g_dbus_connection_call_sync);
REAL(g_dbus_proxy_new_sync);
REAL(g_dbus_proxy_new_for_bus_sync);
GDBusConnection *g_bus_get_sync(int type, void *cancellable, void **error) {
  GDBusConnection *r;
  TIMED(r, GET(g_bus_get_sync)(type, cancellable, error), "g_bus_get_sync", "%d", type);
  return r;
}
GDBusConnection *g_dbus_connection_new_for_address_sync(const char *address, int flags, void *observer,
                                                        void *cancellable, void **error) {
  GDBusConnection *r;
  TIMED(r, GET(g_dbus_connection_new_for_address_sync)(address, flags, observer, cancellable, error),
        "g_dbus_connection_new_for_address_sync", "%s", r ? "ok" : "FAILED");
  return r;
}
GVariant *g_dbus_connection_call_sync(GDBusConnection *c, const char *bus, const char *path, const char *iface,
                                      const char *method, GVariant *params, const void *reply_type, int flags,
                                      int timeout, void *cancellable, void **error) {
  GVariant *r;
  TIMED(r, GET(g_dbus_connection_call_sync)(c, bus, path, iface, method, params, reply_type, flags, timeout,
                                           cancellable, error),
        "g_dbus_connection_call_sync", "%s %s.%s %s", bus ? bus : "-", iface ? iface : "-", method ? method : "-",
        r ? "ok" : "FAILED");
  return r;
}
void *g_dbus_proxy_new_sync(GDBusConnection *c, int flags, void *info, const char *name, const char *path,
                            const char *iface, void *cancellable, void **error) {
  void *r;
  TIMED(r, GET(g_dbus_proxy_new_sync)(c, flags, info, name, path, iface, cancellable, error),
        "g_dbus_proxy_new_sync", "%s %s", name ? name : "-", iface ? iface : "-");
  return r;
}
void *g_dbus_proxy_new_for_bus_sync(int type, int flags, void *info, const char *name, const char *path,
                                    const char *iface, void *cancellable, void **error) {
  void *r;
  TIMED(r, GET(g_dbus_proxy_new_for_bus_sync)(type, flags, info, name, path, iface, cancellable, error),
        "g_dbus_proxy_new_sync", "%s %s", name ? name : "-", iface ? iface : "-");
  return r;
}

/* libdbus (Qt D-Bus resolves it with dlsym) */
void *dbus_bus_get_private(int type, void *error);
void *dbus_connection_open_private(const char *address, void *error);
void *dbus_connection_send_with_reply_and_block(void *conn, void *msg, int timeout, void *error);
REAL(dbus_bus_get_private);
REAL(dbus_connection_open_private);
REAL(dbus_connection_send_with_reply_and_block);
void *dbus_bus_get_private(int type, void *error) {
  void *r;
  TIMED(r, GET(dbus_bus_get_private)(type, error), "dbus_bus_get_private", "%d", type);
  return r;
}
void *dbus_connection_open_private(const char *address, void *error) {
  void *r;
  TIMED(r, GET(dbus_connection_open_private)(address, error), "dbus_connection_open_private", "");
  return r;
}
void *dbus_connection_send_with_reply_and_block(void *conn, void *msg, int timeout, void *error) {
  void *r;
  TIMED(r, GET(dbus_connection_send_with_reply_and_block)(conn, msg, timeout, error), "dbus_call_block", "");
  return r;
}

/* Wayland */
struct wl_message {
  const char *name;
  const char *signature;
  const void **types;
};
struct wl_interface {
  const char *name;
  int version;
  int method_count;
  const struct wl_message *methods;
  int event_count;
  const struct wl_message *events;
};
union wl_argument {
  int32_t i;
  uint32_t u;
  int32_t f;
  const char *s;
  void *o;
  uint32_t n;
  void *a;
  int32_t h;
};
void *wl_display_connect(const char *name);
int wl_display_roundtrip(void *d);
int wl_display_roundtrip_queue(void *d, void *q);
int wl_display_flush(void *d);
void *wl_proxy_marshal_array_flags(void *proxy, uint32_t opcode, const struct wl_interface *iface, uint32_t version,
                                   uint32_t flags, union wl_argument *args);
REAL(wl_display_connect);
REAL(wl_display_roundtrip);
REAL(wl_display_roundtrip_queue);
REAL(wl_display_flush);
REAL(wl_proxy_marshal_array_flags);
static void *display;
void *wl_display_connect(const char *name) {
  void *r;
  TIMED(r, GET(wl_display_connect)(name), "wl_display_connect", "");
  if (!display) display = r;
  return r;
}
int wl_display_roundtrip(void *d) {
  int r;
  TIMED(r, GET(wl_display_roundtrip)(d), "wl_display_roundtrip", "");
  return r;
}
int wl_display_roundtrip_queue(void *d, void *q) {
  int r;
  TIMED(r, GET(wl_display_roundtrip_queue)(d, q), "wl_display_roundtrip", "queue");
  return r;
}

/* Which wl_surfaces are an xdg toplevel's, and which have a buffer
 * attached since their last commit, by object id: Mesa's EGL and Vulkan
 * WSI attach and commit through proxy wrappers of the app's wl_surface,
 * which are other pointers to the same object. Few surfaces exist at
 * start-up. */
#define MAXS 64
static struct {
  uint32_t xdg_surface, surface;
} xdg[MAXS];
static uint32_t toplevel_surfaces[MAXS];
static uint32_t attached[MAXS];
static int nxdg, ntop, nattached, frames;
static pthread_mutex_t wl_lock = PTHREAD_MUTEX_INITIALIZER;
static int in_list(uint32_t *list, int n, uint32_t id) {
  for (int i = 0; i < n; i++)
    if (list[i] == id) return i;
  return -1;
}
uint32_t wl_proxy_get_id(void *proxy);
REAL(wl_proxy_get_id);

void *wl_proxy_marshal_flags(void *proxy, uint32_t opcode, const struct wl_interface *iface, uint32_t version,
                             uint32_t flags, ...) {
  const struct wl_interface *pi = *(const struct wl_interface **)proxy;
  union wl_argument args[20];
  memset(args, 0, sizeof args);
  const char *sig = pi->methods[opcode].signature;
  va_list ap;
  va_start(ap, flags);
  int i = 0;
  for (const char *c = sig; *c && i < 20; c++) {
    switch (*c) {
    case 'i': args[i++].i = va_arg(ap, int32_t); break;
    case 'u': args[i++].u = va_arg(ap, uint32_t); break;
    case 'f': args[i++].f = va_arg(ap, int32_t); break;
    case 's': args[i++].s = va_arg(ap, const char *); break;
    case 'o': args[i++].o = va_arg(ap, void *); break;
    case 'n': args[i++].o = va_arg(ap, void *); break;
    case 'a': args[i++].a = va_arg(ap, void *); break;
    case 'h': args[i++].h = va_arg(ap, int32_t); break;
    default: break;
    }
  }
  va_end(ap);
  const char *pname = pi->name;
  int is_surface = !strcmp(pname, "wl_surface");
  int watched = is_surface || !strcmp(pname, "xdg_wm_base") || !strcmp(pname, "xdg_surface");
  /* The id before the call: a destructor request frees the proxy. */
  uint32_t id = watched ? GET(wl_proxy_get_id)(proxy) : 0;
  uint64_t t = now_ns();
  void *r = GET(wl_proxy_marshal_array_flags)(proxy, opcode, iface, version, flags, args);
  int frame = 0;
  if (!watched) return r;
  pthread_mutex_lock(&wl_lock);
  if (!strcmp(pname, "xdg_wm_base") && opcode == 2 && nxdg < MAXS && r && args[1].o) {
    xdg[nxdg].xdg_surface = GET(wl_proxy_get_id)(r);
    xdg[nxdg++].surface = GET(wl_proxy_get_id)(args[1].o);
  } else if (!strcmp(pname, "xdg_surface") && opcode == 1) {
    for (int k = 0; k < nxdg; k++)
      if (xdg[k].xdg_surface == id && ntop < MAXS) toplevel_surfaces[ntop++] = xdg[k].surface;
    emit(t, "xdg_toplevel", 0, "%u", id);
  } else if (is_surface && opcode == 1) {
    if (args[0].o && nattached < MAXS && in_list(attached, nattached, id) < 0) attached[nattached++] = id;
    emit(t, "attach", 0, "%u %s", id, args[0].o ? "buffer" : "null");
  } else if (is_surface && opcode == 6) {
    int k = in_list(attached, nattached, id);
    int top = in_list(toplevel_surfaces, ntop, id) >= 0;
    if (k >= 0) {
      attached[k] = attached[--nattached];
      emit(t, "commit-buffer", 0, "%u%s", id, top ? " toplevel" : "");
      if (top) frame = ++frames;
    } else {
      emit(t, "commit", 0, "%u%s", id, top ? " toplevel" : "");
    }
  }
  pthread_mutex_unlock(&wl_lock);
  if (frame == 1 && exit_at_frame) {
    void *(*get_display)(void *) = find_real("wl_proxy_get_display");
    void *d = get_display ? get_display(proxy) : display;
    if (d) GET(wl_display_flush)(d);
    emit(now_ns(), "exit-at-frame", 0, "");
    _exit(0);
  }
  return r;
}

/* Blocking waits */
REAL(poll);
REAL(ppoll);
int poll(struct pollfd *fds, nfds_t n, int timeout) {
  uint64_t t = now_ns();
  int r = GET(poll)(fds, n, timeout);
  uint64_t d = now_ns() - t;
  if (d >= 200000) emit(t, "poll", d, "%lu %d", (unsigned long)n, timeout);
  return r;
}
int ppoll(struct pollfd *fds, nfds_t n, const struct timespec *ts, const sigset_t *mask) {
  uint64_t t = now_ns();
  int r = GET(ppoll)(fds, n, ts, mask);
  uint64_t d = now_ns() - t;
  if (d >= 200000) emit(t, "poll", d, "%lu ppoll", (unsigned long)n);
  return r;
}

/* Threads */
REAL(pthread_create);
int pthread_create(pthread_t *th, const pthread_attr_t *attr, void *(*fn)(void *), void *arg) {
  Dl_info info;
  const char *who = "?";
  const char *lib = "?";
  if (dladdr((void *)fn, &info)) {
    if (info.dli_sname) who = info.dli_sname;
    if (info.dli_fname) {
      lib = strrchr(info.dli_fname, '/');
      lib = lib ? lib + 1 : info.dli_fname;
    }
  }
  emit(now_ns(), "pthread_create", 0, "%s %s", lib, who);
  return GET(pthread_create)(th, attr, fn, arg);
}

/* dlsym: hand out the wrappers above for the names they cover, so that
 * calls through function pointers (libepoxy's EGL, fontique's fontconfig,
 * QtDBus's libdbus) are stamped too. Other lookups are a tail call, which
 * keeps RTLD_NEXT relative to the real caller. */
#define H(fn) {#fn, (void *)fn, (void **)&real_##fn}
static struct hook hooks[] = {
    H(eglGetDisplay), H(eglGetPlatformDisplay), H(eglGetPlatformDisplayEXT), H(eglInitialize),
    H(eglCreateContext), H(eglMakeCurrent), H(vkCreateInstance), H(vkEnumeratePhysicalDevices), H(vkCreateDevice),
    H(vkCreateGraphicsPipelines), H(vkEnumerateInstanceExtensionProperties), H(FcInit),
    H(FcInitLoadConfigAndFonts), H(FcConfigGetCurrent), H(FcFontList), H(FcFontSort), H(FcFontMatch),
    H(xkb_context_new), H(xkb_keymap_new_from_string), H(xkb_keymap_new_from_buffer),
    H(xkb_compose_table_new_from_locale), H(dbus_bus_get_private), H(dbus_connection_open_private),
    H(dbus_connection_send_with_reply_and_block),
};
void *dlsym(void *handle, const char *name) {
  if (!real_dlsym) find_real("dlsym");
  if (handle != RTLD_NEXT && handle != RTLD_DEFAULT) {
    for (size_t i = 0; i < sizeof hooks / sizeof hooks[0]; i++) {
      if (!strcmp(name, hooks[i].name)) {
        void *r = real_dlsym(handle, name);
        if (!r) return r;
        if (!*hooks[i].real) *hooks[i].real = r;
        return *hooks[i].real == r ? hooks[i].wrapper : r;
      }
    }
  }
  __attribute__((musttail)) return real_dlsym(handle, name);
}
