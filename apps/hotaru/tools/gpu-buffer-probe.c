/* Linux-only diagnostic, never linked into Hotaru. Qualifies one RGBA DMA-BUF
 * round trip between unshared EGL contexts; it is not a Wayland or speed test.
 * EGL ownership rules: Khronos EGL_MESA_image_dma_buf_export and
 * EGL_EXT_image_dma_buf_import(_modifiers). The process owns every exported fd.
 */
#define _GNU_SOURCE
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES3/gl3.h>
#include <GLES2/gl2ext.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void quoted(const char *s) {
    if (!s) { fputs("null", stdout); return; }
    putchar('"');
    for (const unsigned char *p = (const unsigned char *)s; *p; ++p) {
        if (*p == '"' || *p == '\\') putchar('\\');
        if (*p < 32) printf("\\u%04x", *p); else putchar(*p);
    }
    putchar('"');
}
static void mappings(void) {
    FILE *file = fopen("/proc/self/maps", "r");
    if (!file) return;
    char *line = NULL; size_t size = 0;
    while (getline(&line, &size, file) >= 0) {
        char *path = strchr(line, '/');
        if (path) {
            path[strcspn(path, "\n")] = 0;
            fputs("{\"event\":\"mapping\",\"path\":", stdout); quoted(path); puts("}");
        }
    }
    free(line); fclose(file);
}
static void failure(const char *stage) {
    fputs("{\"event\":\"failure\",\"stage\":", stdout); quoted(stage);
    printf(",\"egl_error\":%u}\n", eglGetError());
    /* Failed runs exit immediately: the OS releases context resources and fds.
     * Successful runs below explicitly exercise the complete cleanup order. */
    mappings(); exit(2);
}
static int extension(const char *list, const char *name) {
    if (!list) return 0;
    size_t len = strlen(name);
    for (const char *p = list; (p = strstr(p, name)); p += len)
        if ((p == list || p[-1] == ' ') && (!p[len] || p[len] == ' ')) return 1;
    return 0;
}
#define LOAD(type, name) type p_##name = (type)eglGetProcAddress(#name); if (!p_##name) failure(#name)

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc != 2) { fprintf(stderr, "usage: gpu-buffer-probe DEVICE_INDEX\n"); return 2; }
    char *end = NULL;
    long selected = strtol(argv[1], &end, 10);
    if (!argv[1][0] || *end || selected < 0 || selected >= 16) return 2;
    LOAD(PFNEGLQUERYDEVICESEXTPROC, eglQueryDevicesEXT);
    LOAD(PFNEGLQUERYDEVICESTRINGEXTPROC, eglQueryDeviceStringEXT);
    LOAD(PFNEGLGETPLATFORMDISPLAYEXTPROC, eglGetPlatformDisplayEXT);
    EGLDeviceEXT devices[16]; EGLint count = 0;
    if (!p_eglQueryDevicesEXT(16, devices, &count) || selected >= count) failure("device enumeration");
    printf("{\"event\":\"device\",\"index\":%ld,\"enumerated\":%d,\"name\":", selected, count);
    quoted(p_eglQueryDeviceStringEXT(devices[selected], EGL_RENDERER_EXT));
    fputs(",\"drm_render_node\":", stdout);
    quoted(p_eglQueryDeviceStringEXT(devices[selected], EGL_DRM_RENDER_NODE_FILE_EXT));
    puts("}"); eglGetError();
    EGLDisplay display = p_eglGetPlatformDisplayEXT(EGL_PLATFORM_DEVICE_EXT, devices[selected], NULL);
    EGLint major, minor;
    if (display == EGL_NO_DISPLAY || !eglInitialize(display, &major, &minor)) failure("initialize");
    const char *extensions = eglQueryString(display, EGL_EXTENSIONS);
    const char *required[] = {"EGL_KHR_surfaceless_context", "EGL_KHR_image_base", "EGL_KHR_gl_texture_2D_image",
        "EGL_MESA_image_dma_buf_export", "EGL_EXT_image_dma_buf_import", "EGL_EXT_image_dma_buf_import_modifiers", "EGL_KHR_fence_sync"};
    for (size_t i = 0; i < sizeof(required) / sizeof(required[0]); ++i)
        if (!extension(extensions, required[i])) failure(required[i]);
    EGLint attrs[] = {EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT,
                     EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8, EGL_NONE};
    EGLConfig config; EGLint configs;
    if (!eglBindAPI(EGL_OPENGL_ES_API) || !eglChooseConfig(display, attrs, &config, 1, &configs) || configs != 1)
        failure("ES3 config");
    EGLint context_attrs[] = {EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE};
    EGLContext producer = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attrs);
    EGLContext consumer = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attrs);
    if (producer == EGL_NO_CONTEXT || consumer == EGL_NO_CONTEXT ||
        !eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, producer)) failure("unshared contexts");

    LOAD(PFNGLGETSTRINGPROC, glGetString);
    LOAD(PFNGLGETERRORPROC, glGetError);
    LOAD(PFNGLGENTEXTURESPROC, glGenTextures);
    LOAD(PFNGLBINDTEXTUREPROC, glBindTexture);
    LOAD(PFNGLTEXPARAMETERIPROC, glTexParameteri);
    LOAD(PFNGLTEXIMAGE2DPROC, glTexImage2D);
    LOAD(PFNGLDELETETEXTURESPROC, glDeleteTextures);
    LOAD(PFNGLGENFRAMEBUFFERSPROC, glGenFramebuffers);
    LOAD(PFNGLBINDFRAMEBUFFERPROC, glBindFramebuffer);
    LOAD(PFNGLFRAMEBUFFERTEXTURE2DPROC, glFramebufferTexture2D);
    LOAD(PFNGLCHECKFRAMEBUFFERSTATUSPROC, glCheckFramebufferStatus);
    LOAD(PFNGLDELETEFRAMEBUFFERSPROC, glDeleteFramebuffers);
    LOAD(PFNGLCLEARCOLORPROC, glClearColor);
    LOAD(PFNGLCLEARPROC, glClear);
    LOAD(PFNGLENABLEPROC, glEnable);
    LOAD(PFNGLSCISSORPROC, glScissor);
    LOAD(PFNGLREADPIXELSPROC, glReadPixels);
    LOAD(PFNGLEGLIMAGETARGETTEXTURE2DOESPROC, glEGLImageTargetTexture2DOES);
    LOAD(PFNEGLCREATEIMAGEKHRPROC, eglCreateImageKHR);
    LOAD(PFNEGLDESTROYIMAGEKHRPROC, eglDestroyImageKHR);
    LOAD(PFNEGLEXPORTDMABUFIMAGEQUERYMESAPROC, eglExportDMABUFImageQueryMESA);
    LOAD(PFNEGLEXPORTDMABUFIMAGEMESAPROC, eglExportDMABUFImageMESA);
    LOAD(PFNEGLCREATESYNCKHRPROC, eglCreateSyncKHR);
    LOAD(PFNEGLCLIENTWAITSYNCKHRPROC, eglClientWaitSyncKHR);
    LOAD(PFNEGLDESTROYSYNCKHRPROC, eglDestroySyncKHR);
    if (!extension((const char *)p_glGetString(GL_EXTENSIONS), "GL_OES_EGL_image")) failure("GL_OES_EGL_image");
    fputs("{\"event\":\"context\",\"vendor\":", stdout); quoted((const char *)p_glGetString(GL_VENDOR));
    fputs(",\"renderer\":", stdout); quoted((const char *)p_glGetString(GL_RENDERER));
    fputs(",\"version\":", stdout); quoted((const char *)p_glGetString(GL_VERSION)); puts("}");

    enum { SIDE = 32 };
    GLuint texture, framebuffer;
    p_glGenTextures(1, &texture); p_glBindTexture(GL_TEXTURE_2D, texture);
    p_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    p_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    p_glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, SIDE, SIDE, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    p_glGenFramebuffers(1, &framebuffer); p_glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    p_glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0);
    if (p_glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) failure("producer framebuffer");
    const unsigned char colors[4][4] = {{255, 0, 0, 255}, {0, 255, 0, 255}, {0, 0, 255, 255}, {255, 255, 255, 255}};
    p_glEnable(GL_SCISSOR_TEST);
    for (int i = 0; i < 4; ++i) {
        p_glScissor((i % 2) * SIDE / 2, (i / 2) * SIDE / 2, SIDE / 2, SIDE / 2);
        p_glClearColor(colors[i][0] / 255.0f, colors[i][1] / 255.0f, colors[i][2] / 255.0f, 1.0f);
        p_glClear(GL_COLOR_BUFFER_BIT);
    }
    if (p_glGetError() != GL_NO_ERROR) failure("producer draw");
    unsigned char initial[SIDE * SIDE * 4];
    p_glReadPixels(0, 0, SIDE, SIDE, GL_RGBA, GL_UNSIGNED_BYTE, initial);
    if (p_glGetError() != GL_NO_ERROR) failure("producer readback");
    for (int y = 0; y < SIDE; ++y) for (int x = 0; x < SIDE; ++x) {
        int quadrant = (x >= SIDE / 2) + 2 * (y >= SIDE / 2);
        if (memcmp(initial + (y * SIDE + x) * 4, colors[quadrant], 4)) failure("producer pixel mismatch");
    }
    puts("{\"event\":\"producer_readback\",\"matched_rgba_bytes\":4096}");
    EGLint image_attrs[] = {EGL_GL_TEXTURE_LEVEL_KHR, 0, EGL_IMAGE_PRESERVED_KHR, EGL_TRUE, EGL_NONE};
    EGLImageKHR source = p_eglCreateImageKHR(display, producer, EGL_GL_TEXTURE_2D_KHR,
                                           (EGLClientBuffer)(uintptr_t)texture, image_attrs);
    if (source == EGL_NO_IMAGE_KHR) failure("source EGLImage");
    int fourcc = 0, planes = 0;
    EGLuint64KHR modifiers[4] = {0};
    if (!p_eglExportDMABUFImageQueryMESA(display, source, &fourcc, &planes, modifiers)) failure("export query");
    printf("{\"event\":\"export_query\",\"fourcc\":%u,\"planes\":%d,\"modifier\":\"0x%016" PRIx64 "\"}\n",
           (unsigned)fourcc, planes, (uint64_t)modifiers[0]);
    /* This probe deliberately qualifies only a single-plane RGBA image. */
    if (planes != 1) failure("single-plane RGBA required");
    int fd = -1; EGLint stride = 0, offset = 0;
    if (!p_eglExportDMABUFImageMESA(display, source, &fd, &stride, &offset) || fd < 0 || stride <= 0 || offset < 0)
        failure("export fd");
    printf("{\"event\":\"export\",\"fd\":%d,\"stride\":%d,\"offset\":%d}\n", fd, stride, offset);

    EGLSyncKHR fence = p_eglCreateSyncKHR(display, EGL_SYNC_FENCE_KHR, NULL);
    if (fence == EGL_NO_SYNC_KHR || p_eglClientWaitSyncKHR(display, fence, EGL_SYNC_FLUSH_COMMANDS_BIT_KHR,
        UINT64_C(5000000000)) != EGL_CONDITION_SATISFIED_KHR) failure("producer fence wait");
    if (!p_eglDestroySyncKHR(display, fence)) failure("destroy fence");
    if (!eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, consumer)) failure("consumer context");
    EGLint import_attrs[] = {EGL_WIDTH, SIDE, EGL_HEIGHT, SIDE, EGL_IMAGE_PRESERVED_KHR, EGL_TRUE,
        EGL_LINUX_DRM_FOURCC_EXT, fourcc,
        EGL_DMA_BUF_PLANE0_FD_EXT, fd, EGL_DMA_BUF_PLANE0_PITCH_EXT, stride, EGL_DMA_BUF_PLANE0_OFFSET_EXT, offset,
        EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT, (EGLint)(uint32_t)modifiers[0],
        EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT, (EGLint)(uint32_t)(modifiers[0] >> 32), EGL_NONE};
    EGLImageKHR imported = p_eglCreateImageKHR(display, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT, NULL, import_attrs);
    if (imported == EGL_NO_IMAGE_KHR) failure("DMA-BUF import");
    close(fd); fd = -1;  /* EGL retains its own backing reference after import. */
    GLuint copy, target;
    p_glGenTextures(1, &copy); p_glBindTexture(GL_TEXTURE_2D, copy);
    p_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    p_glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    p_glEGLImageTargetTexture2DOES(GL_TEXTURE_2D, imported);
    p_glGenFramebuffers(1, &target); p_glBindFramebuffer(GL_FRAMEBUFFER, target);
    p_glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, copy, 0);
    if (p_glGetError() != GL_NO_ERROR || p_glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE)
        failure("imported framebuffer");
    unsigned char pixels[SIDE * SIDE * 4];
    p_glReadPixels(0, 0, SIDE, SIDE, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
    if (p_glGetError() != GL_NO_ERROR) failure("consumer readback");
    for (int y = 0; y < SIDE; ++y) for (int x = 0; x < SIDE; ++x) {
        int quadrant = (x >= SIDE / 2) + 2 * (y >= SIDE / 2);
        if (memcmp(pixels + (y * SIDE + x) * 4, colors[quadrant], 4)) failure("pixel mismatch");
    }
    mappings(); /* Record loaded drivers while both contexts are still alive. */
    p_glDeleteFramebuffers(1, &target); p_glDeleteTextures(1, &copy);
    if (!p_eglDestroyImageKHR(display, imported)) failure("destroy imported image");
    if (!eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, producer)) failure("return producer");
    if (!p_eglDestroyImageKHR(display, source)) failure("destroy source image");
    p_glDeleteFramebuffers(1, &framebuffer); p_glDeleteTextures(1, &texture);
    if (!eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT) ||
        !eglDestroyContext(display, consumer) || !eglDestroyContext(display, producer) || !eglTerminate(display))
        failure("cleanup");
    puts("{\"event\":\"passed\",\"matched_rgba_bytes\":4096,\"contexts_shared\":false,\"synchronization\":\"bounded CPU fence wait\",\"presentation\":false}");
    return 0;
}
