/* PDFium (nixpkgs `pdfium`) through its C API, as a reader draws with it:
 * a form-fill environment, so widgets are drawn with FPDF_FFLDraw after the
 * page, annotations drawn (FPDF_ANNOT), no form-field highlight, on white.
 * PDFBENCH_PDFIUM_TEST=1 instead draws as PDFium's own pdfium_test does for
 * its pixel tests: form fields highlighted (0xFFE4DD, alpha 100) and pages
 * that have transparency drawn on transparent (written over white). */
#include "bench.h"
#include <fpdf_formfill.h>
#include <fpdf_edit.h>
#include <fpdfview.h>
#include <math.h>

int main(int argc, char **argv) {
  if (argc < 2) {
    fprintf(stderr, "usage: %s FILE.pdf [OUT_DIR]\n", argv[0]);
    return 2;
  }
  const char *out_dir = argc > 2 ? argv[2] : NULL;
  int max_pages = bench_max_pages();
  double scale = bench_scale();
  int pdfium_test = getenv("PDFBENCH_PDFIUM_TEST") != NULL;
  setvbuf(stdout, NULL, _IOLBF, 0);

  FPDF_LIBRARY_CONFIG config;
  memset(&config, 0, sizeof config);
  config.version = 2;
  FPDF_InitLibraryWithConfig(&config);

  double t = bench_now_ms();
  FPDF_DOCUMENT doc = FPDF_LoadDocument(argv[1], NULL);
  if (!doc) {
    printf("error\tFPDF_GetLastError %lu\n", FPDF_GetLastError());
    FPDF_DestroyLibrary();
    return 0;
  }
  FPDF_FORMFILLINFO info;
  memset(&info, 0, sizeof info);
  info.version = 1;
  FPDF_FORMHANDLE form = FPDFDOC_InitFormFillEnvironment(doc, &info);
  if (pdfium_test) {
    FPDF_SetFormFieldHighlightColor(form, FPDF_FORMFIELD_UNKNOWN, 0xFFE4DD);
    FPDF_SetFormFieldHighlightAlpha(form, 100);
  }
  int count = FPDF_GetPageCount(doc);
  printf("open\t%.3f\t%d\n", bench_now_ms() - t, count);

  for (int i = 0; i < count && i < max_pages; i++) {
    t = bench_now_ms();
    FPDF_PAGE page = FPDF_LoadPage(doc, i);
    if (!page) {
      printf("fail\t%d\tFPDF_LoadPage\n", i);
      continue;
    }
    FORM_OnAfterLoadPage(page, form);
    int w = (int)(FPDF_GetPageWidthF(page) * scale);
    int h = (int)(FPDF_GetPageHeightF(page) * scale);
    if (w < 1) w = 1;
    if (h < 1) h = 1;
    int alpha = pdfium_test && FPDFPage_HasTransparency(page);
    FPDF_BITMAP bmp = FPDFBitmap_Create(w, h, alpha);
    if (!bmp) {
      printf("fail\t%d\tFPDFBitmap_Create %dx%d\n", i, w, h);
      FORM_OnBeforeClosePage(page, form);
      FPDF_ClosePage(page);
      continue;
    }
    FPDFBitmap_FillRect(bmp, 0, 0, w, h, alpha ? 0x00000000 : 0xFFFFFFFF);
    FPDF_RenderPageBitmap(bmp, page, 0, 0, w, h, 0, FPDF_ANNOT);
    FPDF_FFLDraw(form, bmp, page, 0, 0, w, h, 0, FPDF_ANNOT);
    double took = bench_now_ms() - t;
    printf("page\t%d\t%.3f\t%d\t%d\n", i, took, w, h);
    if (out_dir) {
      /* BGRx or BGRA; transparent pages composited over white. */
      uint8_t *buf = FPDFBitmap_GetBuffer(bmp);
      int stride = FPDFBitmap_GetStride(bmp);
      if (alpha) {
        for (int y = 0; y < h; y++)
          for (int x = 0; x < w; x++) {
            uint8_t *p = buf + (size_t)y * stride + x * 4;
            int a = p[3];
            for (int c = 0; c < 3; c++) p[c] = (uint8_t)((p[c] * a + 255 * (255 - a) + 127) / 255);
          }
      }
      bench_write_ppm(out_dir, i, w, h, buf, stride, 4, 2, 1, 0);
    }
    FPDFBitmap_Destroy(bmp);
    FORM_OnBeforeClosePage(page, form);
    FPDF_ClosePage(page);
  }
  FPDFDOC_ExitFormFillEnvironment(form);
  FPDF_CloseDocument(doc);
  FPDF_DestroyLibrary();
  printf("done\n");
  return 0;
}
