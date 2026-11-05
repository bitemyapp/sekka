/* What the engines' benchmark programs share: arguments, timing, the lines
 * they print and the PPM they may write. Each program takes FILE.pdf
 * [OUT_DIR], opens the file, then draws its first pages (5, or
 * PDFBENCH_PAGES; "all" for every page) at PDFBENCH_SCALE (2: 144 dpi) on
 * white, timing each page from loading it to its finished pixels:
 *
 *   open  <ms> <pages>        (or: error <why>)
 *   page  <index> <ms> <width> <height>
 *   fail  <index> <why>       (a page the engine couldn't draw)
 *   done
 *
 * A page is width = floor(page width in points * scale) pixels, and the same
 * for its height, as hayro sizes it. Pages are written as <index>.ppm in
 * OUT_DIR, outside the timed part. */
#ifndef PDFBENCH_H
#define PDFBENCH_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static inline double bench_now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

static inline int bench_max_pages(void) {
  const char *s = getenv("PDFBENCH_PAGES");
  if (!s) return 5;
  if (!strcmp(s, "all")) return 1 << 30;
  int n = atoi(s);
  return n > 0 ? n : 5;
}

static inline double bench_scale(void) {
  const char *s = getenv("PDFBENCH_SCALE");
  double v = s ? atof(s) : 2.0;
  return v > 0 ? v : 2.0;
}

/* Writes rows of pixels with the given byte offsets of R, G and B in each
 * pixel of `bpp` bytes. */
static inline void bench_write_ppm(const char *dir, int index, int w, int h, const uint8_t *px,
                                   int stride, int bpp, int r, int g, int b) {
  if (!dir) return;
  char path[4096];
  snprintf(path, sizeof path, "%s/%d.ppm", dir, index);
  FILE *f = fopen(path, "wb");
  if (!f) return;
  fprintf(f, "P6\n%d %d\n255\n", w, h);
  uint8_t *row = (uint8_t *)malloc((size_t)w * 3);
  for (int y = 0; y < h; y++) {
    const uint8_t *s = px + (size_t)y * stride;
    for (int x = 0; x < w; x++) {
      row[x * 3 + 0] = s[x * bpp + r];
      row[x * 3 + 1] = s[x * bpp + g];
      row[x * 3 + 2] = s[x * bpp + b];
    }
    fwrite(row, 1, (size_t)w * 3, f);
  }
  free(row);
  fclose(f);
}

#endif
