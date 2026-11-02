/* Poppler (nixpkgs `poppler`) with its Cairo output device, through
 * poppler-glib (as Evince and GNOME's Papers draw): a Cairo RGB24 image
 * surface painted white, scaled, then poppler_page_render (annotations
 * drawn). */
#include "bench.h"
#include <cairo.h>
#include <poppler.h>

int main(int argc, char **argv) {
  if (argc < 2) {
    fprintf(stderr, "usage: %s FILE.pdf [OUT_DIR]\n", argv[0]);
    return 2;
  }
  const char *out_dir = argc > 2 ? argv[2] : NULL;
  int max_pages = bench_max_pages();
  double scale = bench_scale();
  setvbuf(stdout, NULL, _IOLBF, 0);

  double t = bench_now_ms();
  GError *err = NULL;
  char *abs = g_canonicalize_filename(argv[1], NULL);
  char *uri = g_filename_to_uri(abs, NULL, &err);
  PopplerDocument *doc = uri ? poppler_document_new_from_file(uri, NULL, &err) : NULL;
  if (!doc) {
    printf("error\t%s\n", err ? err->message : "poppler_document_new_from_file");
    return 0;
  }
  int count = poppler_document_get_n_pages(doc);
  printf("open\t%.3f\t%d\n", bench_now_ms() - t, count);

  for (int i = 0; i < count && i < max_pages; i++) {
    t = bench_now_ms();
    PopplerPage *page = poppler_document_get_page(doc, i);
    if (!page) {
      printf("fail\t%d\tpoppler_document_get_page\n", i);
      continue;
    }
    double pw, ph;
    poppler_page_get_size(page, &pw, &ph);
    int w = (int)(pw * scale), h = (int)(ph * scale);
    if (w < 1) w = 1;
    if (h < 1) h = 1;
    cairo_surface_t *s = cairo_image_surface_create(CAIRO_FORMAT_RGB24, w, h);
    cairo_t *cr = cairo_create(s);
    cairo_set_source_rgb(cr, 1, 1, 1);
    cairo_paint(cr);
    cairo_scale(cr, scale, scale);
    poppler_page_render(page, cr);
    cairo_destroy(cr);
    cairo_surface_flush(s);
    double took = bench_now_ms() - t;
    if (cairo_surface_status(s) != CAIRO_STATUS_SUCCESS) {
      printf("fail\t%d\tcairo %s\n", i, cairo_status_to_string(cairo_surface_status(s)));
    } else {
      printf("page\t%d\t%.3f\t%d\t%d\n", i, took, w, h);
      if (out_dir)  // RGB24 is B, G, R, x in memory on little-endian.
        bench_write_ppm(out_dir, i, w, h, cairo_image_surface_get_data(s),
                        cairo_image_surface_get_stride(s), 4, 2, 1, 0);
    }
    cairo_surface_destroy(s);
    g_object_unref(page);
  }
  g_object_unref(doc);
  printf("done\n");
  return 0;
}
