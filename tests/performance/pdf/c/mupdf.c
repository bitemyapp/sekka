/* MuPDF (nixpkgs `mupdf`) through its C API: the draw device into an RGB
 * pixmap cleared to white, one thread (no locks given to the context), the
 * page's contents, annotations and widgets (fz_run_page), MuPDF's default
 * anti-aliasing and color management. */
#include "bench.h"
#include <mupdf/fitz.h>

int main(int argc, char **argv) {
  if (argc < 2) {
    fprintf(stderr, "usage: %s FILE.pdf [OUT_DIR]\n", argv[0]);
    return 2;
  }
  const char *out_dir = argc > 2 ? argv[2] : NULL;
  int max_pages = bench_max_pages();
  float scale = (float)bench_scale();
  setvbuf(stdout, NULL, _IOLBF, 0);

  fz_context *ctx = fz_new_context(NULL, NULL, FZ_STORE_DEFAULT);
  if (!ctx) {
    printf("error\tfz_new_context\n");
    return 0;
  }
  fz_register_document_handlers(ctx);
  fz_document *volatile doc = NULL;
  volatile int count = 0;
  double t = bench_now_ms();
  fz_try(ctx) {
    doc = fz_open_document(ctx, argv[1]);
    if (fz_needs_password(ctx, doc)) fz_throw(ctx, FZ_ERROR_GENERIC, "needs a password");
    count = fz_count_pages(ctx, doc);
  }
  fz_catch(ctx) {
    printf("error\t%s\n", fz_caught_message(ctx));
    fz_drop_document(ctx, doc);
    fz_drop_context(ctx);
    return 0;
  }
  printf("open\t%.3f\t%d\n", bench_now_ms() - t, count);

  for (int i = 0; i < count && i < max_pages; i++) {
    fz_page *volatile page = NULL;
    fz_pixmap *volatile pix = NULL;
    fz_device *volatile dev = NULL;
    t = bench_now_ms();
    fz_try(ctx) {
      page = fz_load_page(ctx, doc, i);
      fz_rect r = fz_bound_page(ctx, page);
      int w = (int)((r.x1 - r.x0) * scale);
      int h = (int)((r.y1 - r.y0) * scale);
      if (w < 1) w = 1;
      if (h < 1) h = 1;
      fz_matrix ctm = fz_concat(fz_translate(-r.x0, -r.y0), fz_scale(scale, scale));
      pix = fz_new_pixmap(ctx, fz_device_rgb(ctx), w, h, NULL, 0);
      fz_clear_pixmap_with_value(ctx, pix, 0xff);
      dev = fz_new_draw_device(ctx, fz_identity, pix);
      fz_run_page(ctx, page, dev, ctm, NULL);
      fz_close_device(ctx, dev);
      double took = bench_now_ms() - t;
      printf("page\t%d\t%.3f\t%d\t%d\n", i, took, w, h);
      if (out_dir)
        bench_write_ppm(out_dir, i, w, h, fz_pixmap_samples(ctx, pix), (int)fz_pixmap_stride(ctx, pix), 3, 0, 1, 2);
    }
    fz_always(ctx) {
      fz_drop_device(ctx, dev);
      fz_drop_pixmap(ctx, pix);
      fz_drop_page(ctx, page);
    }
    fz_catch(ctx) {
      printf("fail\t%d\t%s\n", i, fz_caught_message(ctx));
    }
  }
  fz_drop_document(ctx, doc);
  fz_drop_context(ctx);
  printf("done\n");
  return 0;
}
