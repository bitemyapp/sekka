/* Probe the pinned Ghostty page's unique-link capacity through its public C API.
 * Build/run with history-hyperlink-capacity.py. No internal page pointers used. */
#include <ghostty/vt.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(GhosttyResult result) {
    if (result != GHOSTTY_SUCCESS) { fprintf(stderr, "Ghostty result %d\n", result); exit(2); }
}
int main(int argc, char **argv) {
    if (argc != 3) return 2;
    const size_t count = strtoull(argv[2], NULL, 10);
    if (count > 120 * 24) return 2;
    FILE *file = fopen(argv[1], "rb");
    if (!file || fseek(file, 0, SEEK_END)) return 2;
    long length = ftell(file);
    if (length < 0 || fseek(file, 0, SEEK_SET)) return 2;
    uint8_t *data = malloc((size_t)length);
    if (!data || fread(data, 1, (size_t)length, file) != (size_t)length) return 2;
    fclose(file);
    GhosttyTerminal terminal;
    check(ghostty_terminal_new(NULL, &terminal, 120, 24));
    ghostty_terminal_vt_write(terminal, data, (size_t)length);
    free(data);
    printf("{\"links_requested\":%zu,\"mismatches\":[", count);
    size_t mismatches = 0;
    for (size_t index = 0; index < count; index++) {
        GhosttyPoint point = {.tag = GHOSTTY_POINT_TAG_SCREEN,
            .value.coordinate = {.x = index % 120, .y = index / 120}};
        GhosttyGridRef ref = {.size = sizeof ref};
        check(ghostty_terminal_grid_ref(terminal, point, &ref));
        GhosttyCell cell;
        uint32_t codepoint = 0;
        check(ghostty_grid_ref_cell(&ref, &cell));
        check(ghostty_cell_get(cell, GHOSTTY_CELL_DATA_CODEPOINT, &codepoint));
        if (codepoint != 'x') return 3;
        uint8_t uri[128];
        size_t len = 0;
        check(ghostty_grid_ref_hyperlink_uri(&ref, uri, sizeof uri, &len));
        char expected[128];
        size_t expected_len = (size_t)snprintf(expected, sizeof expected, "https://x/%zu", index);
        if (len == expected_len && memcmp(uri, expected, len) == 0) continue;
        if (mismatches++) printf(",");
        printf("{\"index\":%zu,\"row\":%zu,\"col\":%zu,\"codepoint\":%u,\"expected\":\"%s\",\"actual_hex\":\"", index, index / 120, index % 120, codepoint, expected);
        for (size_t byte = 0; byte < len; byte++) printf("%02x", uri[byte]);
        printf("\"}");
    }
    printf("],\"links_preserved\":%zu}\n", count - mismatches);
    ghostty_terminal_free(terminal);
    return 0;
}
