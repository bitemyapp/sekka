// What a compositor feels on a GPU while a model runs there: render a
// 2560x1440 frame of four blended full-screen layers, wait for it to finish,
// every 4.17 ms (240 Hz) for SECONDS; print the frame times as JSON.
// The GPU is chosen by its DRM render node through EGL's device platform.
#define EGL_EGLEXT_PROTOTYPES
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

static int cmp(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

static GLuint shader(GLenum kind, const char *src) {
    GLuint s = glCreateShader(kind);
    glShaderSource(s, 1, &src, NULL);
    glCompileShader(s);
    return s;
}

int main(int argc, char **argv) {
    const char *node = argc > 1 ? argv[1] : "/dev/dri/renderD129";
    const char *label = argc > 2 ? argv[2] : "probe";
    double seconds = argc > 3 ? atof(argv[3]) : 20.0;
    EGLDeviceEXT devs[8];
    EGLint n = 0;
    PFNEGLQUERYDEVICESEXTPROC qd = (void *)eglGetProcAddress("eglQueryDevicesEXT");
    PFNEGLQUERYDEVICESTRINGEXTPROC qs = (void *)eglGetProcAddress("eglQueryDeviceStringEXT");
    PFNEGLGETPLATFORMDISPLAYEXTPROC gp = (void *)eglGetProcAddress("eglGetPlatformDisplayEXT");
    if (!qd || !qs || !gp || !qd(8, devs, &n)) { fprintf(stderr, "no EGL device platform\n"); return 1; }
    EGLDisplay dpy = EGL_NO_DISPLAY;
    for (int i = 0; i < n; i++) {
        const char *r = qs(devs[i], EGL_DRM_RENDER_NODE_FILE_EXT);
        if (r && strcmp(r, node) == 0) dpy = gp(EGL_PLATFORM_DEVICE_EXT, devs[i], NULL);
    }
    if (dpy == EGL_NO_DISPLAY || !eglInitialize(dpy, NULL, NULL)) { fprintf(stderr, "no display for %s\n", node); return 1; }
    eglBindAPI(EGL_OPENGL_ES_API);
    EGLint cfg_attr[] = {EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE};
    EGLConfig cfg;
    EGLint nc;
    eglChooseConfig(dpy, cfg_attr, &cfg, 1, &nc);
    EGLint ctx_attr[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    EGLContext ctx = eglCreateContext(dpy, nc ? cfg : EGL_NO_CONFIG_KHR, EGL_NO_CONTEXT, ctx_attr);
    if (!eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, ctx)) { fprintf(stderr, "no context\n"); return 1; }
    const int W = 2560, H = 1440;
    GLuint tex[2], fbo;
    glGenTextures(2, tex);
    for (int i = 0; i < 2; i++) {
        glBindTexture(GL_TEXTURE_2D, tex[i]);
        glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, W, H, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    }
    glGenFramebuffers(1, &fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex[0], 0);
    GLuint prog = glCreateProgram();
    glAttachShader(prog, shader(GL_VERTEX_SHADER, "attribute vec2 p; varying vec2 uv; void main(){ uv = p * 0.5 + 0.5; gl_Position = vec4(p, 0.0, 1.0); }"));
    glAttachShader(prog, shader(GL_FRAGMENT_SHADER, "precision mediump float; varying vec2 uv; uniform sampler2D t; uniform float k; void main(){ vec4 c = texture2D(t, uv * k); gl_FragColor = vec4(c.rgb * 0.9 + uv.xyx * 0.1, 0.5); }"));
    glBindAttribLocation(prog, 0, "p");
    glLinkProgram(prog);
    glUseProgram(prog);
    GLint uk = glGetUniformLocation(prog, "k");
    static const float quad[] = {-1, -1, 1, -1, -1, 1, 1, 1};
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, quad);
    glEnableVertexAttribArray(0);
    glViewport(0, 0, W, H);
    glEnable(GL_BLEND);
    glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    glBindTexture(GL_TEXTURE_2D, tex[1]);
    const double period = 1.0 / 240.0;
    int cap = (int)(seconds / period) + 16, count = 0;
    double *ft = malloc(sizeof(double) * cap);
    for (int i = 0; i < 60; i++) { glClear(GL_COLOR_BUFFER_BIT); glDrawArrays(GL_TRIANGLE_STRIP, 0, 4); glFinish(); }
    double end = now() + seconds, next = now();
    while (now() < end && count < cap) {
        double t0 = now();
        glClearColor(0.1f, 0.1f, 0.15f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
        for (int l = 0; l < 4; l++) { glUniform1f(uk, 1.0f + 0.01f * l); glDrawArrays(GL_TRIANGLE_STRIP, 0, 4); }
        glFinish();
        ft[count++] = (now() - t0) * 1000.0;
        next += period;
        double wait = next - now();
        if (wait > 0) { struct timespec ts = {0, (long)(wait * 1e9)}; nanosleep(&ts, NULL); } else next = now();
    }
    qsort(ft, count, sizeof(double), cmp);
    int over4 = 0, over8 = 0;
    for (int i = 0; i < count; i++) { over4 += ft[i] > 4.17; over8 += ft[i] > 8.33; }
    printf("{\"label\": \"%s\", \"frames\": %d, \"p50_ms\": %.3f, \"p99_ms\": %.3f, \"p999_ms\": %.3f, \"max_ms\": %.2f, \"over_240hz\": %d, \"over_120hz\": %d}\n",
           label, count, ft[count / 2], ft[(int)(count * 0.99)], ft[(int)(count * 0.999)], ft[count - 1], over4, over8);
    return 0;
}
