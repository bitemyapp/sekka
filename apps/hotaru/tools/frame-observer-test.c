/* Synthetic pixel tests for the actual observer decoder, including torn
 * markers, wrong CRC, pixel formats and output inversion. No compositor. */
#define main observer_main
#include "frame-observer.c"
#undef main
#include <assert.h>

static void put_pixel(int x, int y, uint32_t color) {
    if (flags & ZWLR_SCREENCOPY_FRAME_V1_FLAGS_Y_INVERT) y = (int)height - 1 - y;
    if (format == WL_SHM_FORMAT_XBGR8888 || format == WL_SHM_FORMAT_ABGR8888)
        color = ((color & 255) << 16) | (color & 0xff00) | ((color >> 16) & 255);
    memcpy(pixels + (size_t)y * stride + (size_t)x * 4, &color, 4);
}
static void paint_marker(int x, int y, uint32_t sequence, bool bad_crc) {
    uint64_t bits = (uint64_t)sequence << 32 | (checksum(sequence) ^ (bad_crc ? 1u : 0u));
    for (int cell = 0; cell < MARKER_CELLS; ++cell) {
        uint32_t color = cell < 6 ? guard[cell] : cell >= 70 ? guard[75 - cell] : bit_color[(bits >> (69 - cell)) & 1];
        for (int dy = 0; dy < gh; ++dy) for (int dx = 0; dx < gw; ++dx)
            put_pixel(x + cell * gw + dx, y + dy, color);
    }
}
int main(void) {
    width = 900; height = 500; stride = width * 4 + 16;
    pixels = calloc(height, stride); assert(pixels);
    nonce = 0x29312982;
    /* Cross-checked by Python zlib over big-endian sequence and nonce. */
    printf("checksum=%08x\n", checksum(0x12345678));
    const uint32_t formats[] = {WL_SHM_FORMAT_XRGB8888, WL_SHM_FORMAT_ARGB8888, WL_SHM_FORMAT_XBGR8888, WL_SHM_FORMAT_ABGR8888};
    unsigned cases = 0;
    for (size_t f = 0; f < 4; ++f) for (unsigned flip = 0; flip < 2; ++flip) {
        format = formats[f]; flags = flip; gw = 9; gh = 17; gx = 13; gy = 7; bottom_y = gy + 23 * gh;
        memset(pixels, 0, (size_t)height * stride);
        paint_marker(gx, gy, 0x12345678, false); paint_marker(gx, bottom_y, 0x12345678, false);
        uint32_t sequence;
        assert(decode(&sequence) && sequence == 0x12345678); ++cases;
        int x = gx; gx = -1;
        assert(decode(&sequence) && sequence == 0x12345678 && gx == x); ++cases;
        for (int cell = 0; cell < MARKER_CELLS; ++cell) {
            uint32_t old = pixel(gx + cell * gw + gw / 2, gy + gh / 2);
            put_pixel(gx + cell * gw + gw / 2, gy + gh / 2, old ^ 1);
            assert(!decode(&sequence)); ++cases;
            put_pixel(gx + cell * gw + gw / 2, gy + gh / 2, old);
        }
        paint_marker(gx, bottom_y, 0x12345679, false);
        assert(!decode(&sequence)); ++cases;
        paint_marker(gx, bottom_y, 0x12345678, true);
        assert(!decode(&sequence)); ++cases;
        paint_marker(gx, bottom_y, 0x12345678, false);
        nonce ^= 1; assert(!decode(&sequence)); ++cases; nonce ^= 1;
        gw = 100; assert(!decode(&sequence)); ++cases;
    }
    free(pixels); pixels = NULL;
    printf("passed %u synthetic marker checks\n", cases);
    return 0;
}
