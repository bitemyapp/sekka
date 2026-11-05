// Poppler (nixpkgs `poppler`) with its Splash rasterizer, through
// poppler-cpp's page_renderer (as pdftoppm draws): ARGB32 on white paper,
// anti-aliased vectors and text, no text hinting, Poppler's own page size at
// the resolution (it rounds up where the others round down).
#include "bench.h"
#include <memory>
#include <poppler-document.h>
#include <poppler-image.h>
#include <poppler-page-renderer.h>
#include <poppler-page.h>

int main(int argc, char **argv) {
  if (argc < 2) {
    fprintf(stderr, "usage: %s FILE.pdf [OUT_DIR]\n", argv[0]);
    return 2;
  }
  const char *out_dir = argc > 2 ? argv[2] : nullptr;
  int max_pages = bench_max_pages();
  double dpi = 72.0 * bench_scale();
  setvbuf(stdout, nullptr, _IOLBF, 0);
  poppler::set_debug_error_function([](const std::string &, void *) {}, nullptr);

  double t = bench_now_ms();
  std::unique_ptr<poppler::document> doc(poppler::document::load_from_file(argv[1]));
  if (!doc) {
    printf("error\tload_from_file\n");
    return 0;
  }
  if (doc->is_locked()) {
    printf("error\tneeds a password\n");
    return 0;
  }
  int count = doc->pages();
  printf("open\t%.3f\t%d\n", bench_now_ms() - t, count);

  poppler::page_renderer renderer;
  renderer.set_render_hint(poppler::page_renderer::antialiasing, true);
  renderer.set_render_hint(poppler::page_renderer::text_antialiasing, true);
  renderer.set_paper_color(0xffffffff);
  renderer.set_image_format(poppler::image::format_argb32);
  for (int i = 0; i < count && i < max_pages; i++) {
    t = bench_now_ms();
    std::unique_ptr<poppler::page> page(doc->create_page(i));
    if (!page) {
      printf("fail\t%d\tcreate_page\n", i);
      continue;
    }
    poppler::image img = renderer.render_page(page.get(), dpi, dpi);
    double took = bench_now_ms() - t;
    if (!img.is_valid()) {
      printf("fail\t%d\trender_page\n", i);
      continue;
    }
    printf("page\t%d\t%.3f\t%d\t%d\n", i, took, img.width(), img.height());
    if (out_dir)  // ARGB32 is B, G, R, A in memory on little-endian.
      bench_write_ppm(out_dir, i, img.width(), img.height(), (const uint8_t *)img.const_data(),
                      img.bytes_per_row(), 4, 2, 1, 0);
  }
  printf("done\n");
  return 0;
}
