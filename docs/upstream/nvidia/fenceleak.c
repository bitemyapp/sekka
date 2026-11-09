// Creates and destroys sync objects in a loop, printing the process's
// anonymous RSS. No window system: EGL_EXT_platform_device and a
// surfaceless GLES 3 context.
//
//   cc -O2 -o fenceleak fenceleak.c -ldl
//   ./fenceleak [pairs, default 5000000] [device index, default 0] [gl|egl|egl-native|flush] [work]
//
//   gl          glFenceSync + glDeleteSync
//   egl         eglCreateSync(EGL_SYNC_FENCE) + eglDestroySync
//   egl-native  eglCreateSync(EGL_SYNC_NATIVE_FENCE_ANDROID) + glFlush +
//               eglDupNativeFenceFDANDROID + close + eglDestroySync
//   flush       glFlush only, no sync object (a control)
//   work        clear a 64x64 renderbuffer before each sync
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void *EGLDisplay, *EGLContext, *EGLConfig, *EGLDeviceEXT, *EGLSurface;
typedef int EGLint;
typedef unsigned EGLBoolean, EGLenum;
typedef struct __GLsync *GLsync;
typedef unsigned GLenum, GLbitfield, GLuint;
typedef int GLint, GLsizei;

#define EGL_PLATFORM_DEVICE_EXT 0x313F
#define EGL_NONE 0x3038
#define EGL_OPENGL_ES_API 0x30A0
#define EGL_CONTEXT_MAJOR_VERSION 0x3098
#define EGL_RENDERABLE_TYPE 0x3040
#define EGL_OPENGL_ES3_BIT 0x40
#define EGL_SURFACE_TYPE 0x3033
#define EGL_VENDOR 0x3053
#define EGL_VERSION 0x3054
#define GL_SYNC_GPU_COMMANDS_COMPLETE 0x9117
#define EGL_SYNC_FENCE 0x30F9
#define EGL_SYNC_NATIVE_FENCE_ANDROID 0x3144
typedef void *EGLSync;
typedef long EGLAttrib;
#define GL_VENDOR 0x1F00
#define GL_VERSION 0x1F02

static long rss_anon_kb(void) {
    FILE *f = fopen("/proc/self/status", "r");
    char line[256];
    long v = -1;
    while (f && fgets(line, sizeof line, f))
        if (!strncmp(line, "RssAnon:", 8)) v = strtol(line + 8, NULL, 10);
    if (f) fclose(f);
    return v;
}

#define LOAD(name) name = (void *)getproc(#name); if (!name) { fprintf(stderr, "no %s\n", #name); return 1; }

int main(int argc, char **argv) {
    long pairs = argc > 1 ? atol(argv[1]) : 5000000;
    int index = argc > 2 ? atoi(argv[2]) : 0;
    const char *mode = argc > 3 ? argv[3] : "gl";
    void *egl = dlopen("libEGL.so.1", RTLD_NOW);
    if (!egl) { fprintf(stderr, "%s\n", dlerror()); return 1; }
    void *(*getproc)(const char *) = dlsym(egl, "eglGetProcAddress");

    EGLBoolean (*eglQueryDevicesEXT)(EGLint, EGLDeviceEXT *, EGLint *);
    EGLDisplay (*eglGetPlatformDisplayEXT)(EGLenum, void *, const EGLint *);
    EGLBoolean (*eglInitialize)(EGLDisplay, EGLint *, EGLint *);
    EGLBoolean (*eglBindAPI)(EGLenum);
    EGLBoolean (*eglChooseConfig)(EGLDisplay, const EGLint *, EGLConfig *, EGLint, EGLint *);
    EGLContext (*eglCreateContext)(EGLDisplay, EGLConfig, EGLContext, const EGLint *);
    EGLBoolean (*eglMakeCurrent)(EGLDisplay, EGLSurface, EGLSurface, EGLContext);
    const char *(*eglQueryString)(EGLDisplay, EGLint);
    GLsync (*glFenceSync)(GLenum, GLbitfield);
    void (*glDeleteSync)(GLsync);
    const unsigned char *(*glGetString)(GLenum);
    EGLSync (*eglCreateSync)(EGLDisplay, EGLenum, const EGLAttrib *);
    EGLBoolean (*eglDestroySync)(EGLDisplay, EGLSync);
    EGLint (*eglDupNativeFenceFDANDROID)(EGLDisplay, EGLSync);
    void (*glFlush)(void);
    void (*glGenFramebuffers)(GLsizei, GLuint *);
    void (*glGenRenderbuffers)(GLsizei, GLuint *);
    void (*glBindFramebuffer)(GLenum, GLuint);
    void (*glBindRenderbuffer)(GLenum, GLuint);
    void (*glRenderbufferStorage)(GLenum, GLenum, GLsizei, GLsizei);
    void (*glFramebufferRenderbuffer)(GLenum, GLenum, GLenum, GLuint);
    void (*glClearColor)(float, float, float, float);
    void (*glClear)(GLbitfield);
    LOAD(eglQueryDevicesEXT) LOAD(eglGetPlatformDisplayEXT) LOAD(eglInitialize) LOAD(eglBindAPI)
    LOAD(eglChooseConfig) LOAD(eglCreateContext) LOAD(eglMakeCurrent) LOAD(eglQueryString)
    LOAD(glFenceSync) LOAD(glDeleteSync) LOAD(glGetString) LOAD(eglCreateSync) LOAD(eglDestroySync)
    LOAD(eglDupNativeFenceFDANDROID) LOAD(glFlush) LOAD(glGenFramebuffers) LOAD(glGenRenderbuffers)
    LOAD(glBindFramebuffer) LOAD(glBindRenderbuffer) LOAD(glRenderbufferStorage) LOAD(glFramebufferRenderbuffer)
    LOAD(glClearColor) LOAD(glClear)

    EGLDeviceEXT devs[16];
    EGLint ndev = 0;
    if (!eglQueryDevicesEXT(16, devs, &ndev) || index >= ndev) { fprintf(stderr, "no EGL device %d\n", index); return 1; }
    EGLDisplay dpy = eglGetPlatformDisplayEXT(EGL_PLATFORM_DEVICE_EXT, devs[index], NULL);
    EGLint major, minor;
    if (!dpy || !eglInitialize(dpy, &major, &minor)) { fprintf(stderr, "eglInitialize failed\n"); return 1; }
    eglBindAPI(EGL_OPENGL_ES_API);
    EGLint cfg_attrs[] = {EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT, EGL_SURFACE_TYPE, 0, EGL_NONE};
    EGLConfig cfg;
    EGLint ncfg = 0;
    if (!eglChooseConfig(dpy, cfg_attrs, &cfg, 1, &ncfg) || ncfg < 1) { fprintf(stderr, "no config\n"); return 1; }
    EGLint ctx_attrs[] = {EGL_CONTEXT_MAJOR_VERSION, 3, EGL_NONE};
    EGLContext ctx = eglCreateContext(dpy, cfg, NULL, ctx_attrs);
    if (!ctx || !eglMakeCurrent(dpy, NULL, NULL, ctx)) { fprintf(stderr, "no surfaceless context\n"); return 1; }
    printf("EGL %s %s; GL %s %s\n", eglQueryString(dpy, EGL_VENDOR), eglQueryString(dpy, EGL_VERSION),
           glGetString(GL_VENDOR), glGetString(GL_VERSION));

    long base = rss_anon_kb();
    printf("%s: start RssAnon %ld kB\n", mode, base);
    int m = !strcmp(mode, "egl") ? 1 : !strcmp(mode, "egl-native") ? 2 : !strcmp(mode, "flush") ? 3 : 0;
    int work = argc > 4 && !strcmp(argv[4], "work");
    if (work) {
        GLuint fb, rb;
        glGenFramebuffers(1, &fb);
        glGenRenderbuffers(1, &rb);
        glBindRenderbuffer(0x8D41 /* GL_RENDERBUFFER */, rb);
        glRenderbufferStorage(0x8D41, 0x8058 /* GL_RGBA8 */, 64, 64);
        glBindFramebuffer(0x8D40 /* GL_FRAMEBUFFER */, fb);
        glFramebufferRenderbuffer(0x8D40, 0x8CE0 /* GL_COLOR_ATTACHMENT0 */, 0x8D41, rb);
        printf("clearing a 64x64 renderbuffer before each sync\n");
    }
    long step = pairs / 10 ? pairs / 10 : 1;
    for (long i = 1; i <= pairs; i++) {
        if (work) {
            glClearColor((i & 255) / 255.0f, 0, 0, 1);
            glClear(0x4000 /* GL_COLOR_BUFFER_BIT */);
        }
        if (m == 3) {
            glFlush();
        } else if (m == 0) {
            glDeleteSync(glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0));
        } else {
            EGLSync s = eglCreateSync(dpy, m == 1 ? EGL_SYNC_FENCE : EGL_SYNC_NATIVE_FENCE_ANDROID, NULL);
            if (!s) { fprintf(stderr, "eglCreateSync failed\n"); return 1; }
            if (m == 2) {
                glFlush();
                int fd = eglDupNativeFenceFDANDROID(dpy, s);
                if (fd >= 0) close(fd);
            }
            eglDestroySync(dpy, s);
        }
        if (i % step == 0) {
            long now = rss_anon_kb();
            printf("%9ld pairs: RssAnon %ld kB (+%.1f bytes per pair)\n", i, now, (now - base) * 1024.0 / i);
            fflush(stdout);
        }
    }
    return 0;
}
