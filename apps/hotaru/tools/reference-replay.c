/* Development-only Linux benchmark adapter for the external Ghostty oracle.
 * This file is never linked into Hotaru. Ghostty's implementation remains in
 * the pinned external libghostty-vt, with its own MIT license and provenance.
 */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <ghostty/vt.h>

static void fail(const char *message) {
    fprintf(stderr, "ghostty-reference-replay: %s\n", message);
    exit(1);
}

static void check(GhosttyResult result) {
    if (result != GHOSTTY_SUCCESS) {
        fprintf(stderr, "ghostty-reference-replay: API error %d\n", result);
        exit(1);
    }
}

static uint64_t now_ns(void) {
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value)) fail("clock_gettime failed");
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}

static unsigned number(const char *text) {
    char *end;
    errno = 0;
    unsigned long value = strtoul(text, &end, 10);
    if (errno || !*text || *end || value > UINT32_MAX || value == 0)
        fail("expected positive bounded integer");
    return (unsigned)value;
}

struct replies {
    uint8_t *data;
    size_t length, capacity;
};

static void write_pty(GhosttyTerminal terminal, void *userdata,
                      const uint8_t *data, size_t len) {
    (void)terminal;
    struct replies *replies = userdata;
    if (len > SIZE_MAX - replies->length) fail("reply size overflow");
    size_t need = replies->length + len;
    if (need > replies->capacity) {
        size_t capacity = replies->capacity;
        if (capacity <= SIZE_MAX / 2) capacity *= 2;
        if (capacity < need) capacity = need;
        if (capacity < 8) capacity = 8;
        uint8_t *allocation = realloc(replies->data, capacity);
        if (!allocation) fail("reply allocation failed");
        replies->data = allocation;
        replies->capacity = capacity;
    }
    if (len) memcpy(replies->data + replies->length, data, len);
    replies->length = need;
}

static void configure(GhosttyTerminal terminal, struct replies *replies) {
    const size_t zero = 0;
    check(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, &zero));
    check(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_USERDATA, replies));
    check(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_WRITE_PTY, (const void *)write_pty));
    GhosttyColorRgb palette[256] = {
        {0,0,0}, {205,0,0}, {0,205,0}, {205,205,0},
        {0,0,238}, {205,0,205}, {0,205,205}, {229,229,229},
        {127,127,127}, {255,0,0}, {0,255,0}, {255,255,0},
        {92,92,255}, {255,0,255}, {0,255,255}, {255,255,255}
    };
    const uint8_t levels[] = {0,95,135,175,215,255};
    for (unsigned i = 0; i < 216; ++i)
        palette[16+i] = (GhosttyColorRgb){levels[i/36],levels[(i/6)%6],levels[i%6]};
    for (unsigned i = 0; i < 24; ++i) {
        uint8_t value = (uint8_t)(8 + i*10);
        palette[232+i] = (GhosttyColorRgb){value,value,value};
    }
    const GhosttyColorRgb foreground = {229,229,229}, background = {0,0,0};
    check(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, palette));
    check(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, &foreground));
    check(ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, &background));
}

static void array(const char *name, const uint64_t *values, unsigned count) {
    printf("\"%s\":[", name);
    for (unsigned i = 0; i < count; ++i)
        printf("%s%" PRIu64, i ? "," : "", values[i]);
    putchar(']');
}

int main(int argc, char **argv) {
    const char *path = NULL;
    unsigned rows = 80, cols = 120, chunk = 65536, iterations = 15;
    int bench_json = 0;
    for (int i = 1; i < argc; ++i) {
        const char *option = argv[i];
        if (!strcmp(option, "--bench-json")) { bench_json = 1; continue; }
        if (i + 1 >= argc) fail("option requires a value");
        const char *value = argv[++i];
        if (!strcmp(option, "--data")) path = value;
        else if (!strcmp(option, "--rows")) rows = number(value);
        else if (!strcmp(option, "--cols")) cols = number(value);
        else if (!strcmp(option, "--chunk")) chunk = number(value);
        else if (!strcmp(option, "--iterations")) iterations = number(value);
        else fail("unknown option");
    }
    if (!path || !bench_json || rows > 4096 || cols > 8192 || iterations > 10000)
        fail("required: --data FILE --bench-json; dimensions/iterations must be bounded");
    FILE *file = fopen(path, "rb");
    if (!file) fail("cannot open input");
    if (fseek(file, 0, SEEK_END)) fail("input must be seekable");
    long end = ftell(file);
    if (end < 0 || fseek(file, 0, SEEK_SET)) fail("cannot size input");
    size_t length = (size_t)end;
    uint8_t *bytes = malloc(length ? length : 1);
    if (!bytes) fail("input allocation failed");
    if (fread(bytes, 1, length, file) != length) fail("short input read");
    if (fclose(file)) fail("cannot close input");
    uint64_t *setup = calloc(iterations, sizeof(uint64_t));
    uint64_t *parse = calloc(iterations, sizeof(uint64_t));
    uint64_t *teardown = calloc(iterations, sizeof(uint64_t));
    uint64_t *response_bytes = calloc(iterations, sizeof(uint64_t));
    if (!setup || !parse || !teardown || !response_bytes) fail("measurement allocation failed");
    for (unsigned i = 0; i < iterations; ++i) {
        uint64_t start = now_ns();
        struct replies replies = {0};
        GhosttyTerminal terminal;
        check(ghostty_terminal_new(NULL, &terminal, (uint16_t)cols, (uint16_t)rows));
        configure(terminal, &replies);
        setup[i] = now_ns() - start;
        start = now_ns();
        for (size_t offset = 0; offset < length;) {
            size_t take = length - offset < chunk ? length - offset : chunk;
            ghostty_terminal_vt_write(terminal, bytes + offset, take);
            offset += take;
        }
        parse[i] = now_ns() - start;
        /* The foreign library call is externally observable and cannot be
         * optimized away. Validate processing did not fail before recording. */
        bool processing_error = false;
        check(ghostty_terminal_get(terminal, GHOSTTY_TERMINAL_DATA_VT_PROCESSING_ERROR, &processing_error));
        if (processing_error) fail("terminal processing error invalidates timing");
        response_bytes[i] = replies.length;
        start = now_ns();
        ghostty_terminal_free(terminal);
        free(replies.data);
        teardown[i] = now_ns() - start;
    }
    printf("{\"schema\":1,\"engine\":\"ghostty\",\"timed_scope\":\"feed-only\","
           "\"input_bytes\":%zu,\"rows\":%u,\"cols\":%u,\"scrollback\":0,\"chunk_bytes\":%u,",
           length, rows, cols, chunk);
    array("samples_ns", parse, iterations); putchar(',');
    array("setup_ns", setup, iterations); putchar(',');
    array("teardown_ns", teardown, iterations); putchar(',');
    array("response_bytes", response_bytes, iterations); puts("}");
    free(response_bytes); free(teardown); free(parse); free(setup); free(bytes);
    return 0;
}
