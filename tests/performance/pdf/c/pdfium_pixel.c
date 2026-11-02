/* PDFium drawing a PDF as its own pdfium_test does for PDFium's pixel tests
 * (testing/resources/pixel), so its pages can be compared with the tests'
 * expected PNGs as a control: every page at 72 dpi, a form-fill environment
 * with fields highlighted (0xFFE4DD, alpha 100), FPDF_ANNOT, pages with
 * transparency drawn on transparent (written here over white), and fonts
 * from a font directory (argument 3) with --croscore-font-names' renaming
 * (Arial/Helvetica/Calibri to Arimo, Times or no name to Tinos, Courier to
 * Cousine, keeping Bold and Italic) unless PDFBENCH_NO_RENAME is set. Not
 * done: --send-events (tests with .evt files are left out), JavaScript (this
 * PDFium is built without V8) and --time. Prints as the benchmark programs.
 *
 *   pdfbench-pdfium-pixel FILE.pdf OUT_DIR [FONT_DIR] */
#include "bench.h"
#include <fpdf_edit.h>
#include <fpdf_formfill.h>
#include <fpdf_sysfontinfo.h>
#include <fpdfview.h>

typedef struct {
  FPDF_SYSFONTINFO base;
  FPDF_SYSFONTINFO *impl;
} Renamer;

static void rename_face(const char *face, char *out, size_t n) {
  const char *base = NULL;
  if (!face) face = "";
  if (strstr(face, "Arial") || strstr(face, "Calibri") || strstr(face, "Helvetica"))
    base = "Arimo";
  else if (!*face || strstr(face, "Times"))
    base = "Tinos";
  else if (strstr(face, "Courier"))
    base = "Cousine";
  if (!base) {
    snprintf(out, n, "%s", face);
    return;
  }
  snprintf(out, n, "%s%s%s", base, strstr(face, "Bold") ? " Bold" : "",
           (strstr(face, "Italic") || strstr(face, "Oblique")) ? " Italic" : "");
}

static FPDF_SYSFONTINFO *impl_of(FPDF_SYSFONTINFO *p) { return ((Renamer *)p)->impl; }
static void r_release(FPDF_SYSFONTINFO *p) { impl_of(p)->Release(impl_of(p)); }
static void r_enum(FPDF_SYSFONTINFO *p, void *m) { impl_of(p)->EnumFonts(impl_of(p), m); }
static void *r_map(FPDF_SYSFONTINFO *p, int weight, FPDF_BOOL italic, int charset, int pitch,
                   const char *face, FPDF_BOOL *exact) {
  char buf[512];
  rename_face(face, buf, sizeof buf);
  return impl_of(p)->MapFont(impl_of(p), weight, italic, charset, pitch, buf, exact);
}
static void *r_get(FPDF_SYSFONTINFO *p, const char *face) {
  char buf[512];
  rename_face(face, buf, sizeof buf);
  return impl_of(p)->GetFont(impl_of(p), buf);
}
static unsigned long r_data(FPDF_SYSFONTINFO *p, void *f, unsigned int t, unsigned char *b,
                            unsigned long n) {
  return impl_of(p)->GetFontData(impl_of(p), f, t, b, n);
}
static unsigned long r_name(FPDF_SYSFONTINFO *p, void *f, char *b, unsigned long n) {
  return impl_of(p)->GetFaceName(impl_of(p), f, b, n);
}
static int r_charset(FPDF_SYSFONTINFO *p, void *f) { return impl_of(p)->GetFontCharset(impl_of(p), f); }
static void r_delete(FPDF_SYSFONTINFO *p, void *f) { impl_of(p)->DeleteFont(impl_of(p), f); }

int main(int argc, char **argv) {
  if (argc < 3) {
    fprintf(stderr, "usage: %s FILE.pdf OUT_DIR [FONT_DIR]\n", argv[0]);
    return 2;
  }
  const char *out_dir = argv[2];
  setvbuf(stdout, NULL, _IOLBF, 0);

  const char *paths[2] = {argc > 3 ? argv[3] : NULL, NULL};
  FPDF_LIBRARY_CONFIG config;
  memset(&config, 0, sizeof config);
  config.version = 2;
  config.m_pUserFontPaths = paths[0] ? paths : NULL;
  FPDF_InitLibraryWithConfig(&config);
  Renamer renamer;
  memset(&renamer, 0, sizeof renamer);
  if (paths[0] && !getenv("PDFBENCH_NO_RENAME")) {
    renamer.impl = FPDF_GetDefaultSystemFontInfo();
    renamer.base.version = 1;
    renamer.base.Release = r_release;
    renamer.base.EnumFonts = r_enum;
    renamer.base.MapFont = r_map;
    renamer.base.GetFont = r_get;
    renamer.base.GetFontData = r_data;
    renamer.base.GetFaceName = r_name;
    renamer.base.GetFontCharset = r_charset;
    renamer.base.DeleteFont = r_delete;
    FPDF_SetSystemFontInfo(&renamer.base);
  }

  double t = bench_now_ms();
  FPDF_DOCUMENT doc = FPDF_LoadDocument(argv[1], NULL);
  if (!doc) {
    printf("error\tFPDF_GetLastError %lu\n", FPDF_GetLastError());
    return 0;
  }
  FPDF_FORMFILLINFO info;
  memset(&info, 0, sizeof info);
  info.version = 1;
  FPDF_FORMHANDLE form = FPDFDOC_InitFormFillEnvironment(doc, &info);
  FPDF_SetFormFieldHighlightColor(form, FPDF_FORMFIELD_UNKNOWN, 0xFFE4DD);
  FPDF_SetFormFieldHighlightAlpha(form, 100);
  FORM_DoDocumentOpenAction(form);
  int count = FPDF_GetPageCount(doc);
  printf("open\t%.3f\t%d\n", bench_now_ms() - t, count);

  for (int i = 0; i < count; i++) {
    t = bench_now_ms();
    FPDF_PAGE page = FPDF_LoadPage(doc, i);
    if (!page) {
      printf("fail\t%d\tFPDF_LoadPage\n", i);
      continue;
    }
    FORM_OnAfterLoadPage(page, form);
    FORM_DoPageAAction(page, form, FPDFPAGE_AACTION_OPEN);
    int w = (int)FPDF_GetPageWidthF(page);
    int h = (int)FPDF_GetPageHeightF(page);
    int alpha = FPDFPage_HasTransparency(page);
    FPDF_BITMAP bmp = FPDFBitmap_Create(w, h, alpha);
    if (!bmp) {
      printf("fail\t%d\tFPDFBitmap_Create %dx%d\n", i, w, h);
    } else {
      FPDFBitmap_FillRect(bmp, 0, 0, w, h, alpha ? 0x00000000 : 0xFFFFFFFF);
      FPDF_RenderPageBitmap(bmp, page, 0, 0, w, h, 0, FPDF_ANNOT);
      FPDF_FFLDraw(form, bmp, page, 0, 0, w, h, 0, FPDF_ANNOT);
      printf("page\t%d\t%.3f\t%d\t%d\t%d\n", i, bench_now_ms() - t, w, h, alpha);
      uint8_t *buf = FPDFBitmap_GetBuffer(bmp);
      int stride = FPDFBitmap_GetStride(bmp);
      if (alpha)
        for (int y = 0; y < h; y++)
          for (int x = 0; x < w; x++) {
            uint8_t *p = buf + (size_t)y * stride + x * 4;
            int a = p[3];
            for (int c = 0; c < 3; c++) p[c] = (uint8_t)((p[c] * a + 255 * (255 - a) + 127) / 255);
          }
      bench_write_ppm(out_dir, i, w, h, buf, stride, 4, 2, 1, 0);
      FPDFBitmap_Destroy(bmp);
    }
    FORM_DoPageAAction(page, form, FPDFPAGE_AACTION_CLOSE);
    FORM_OnBeforeClosePage(page, form);
    FPDF_ClosePage(page);
  }
  FPDFDOC_ExitFormFillEnvironment(form);
  FPDF_CloseDocument(doc);
  if (renamer.impl) {
    FPDF_SetSystemFontInfo(NULL);
    FPDF_FreeDefaultSystemFontInfo(renamer.impl);
  }
  FPDF_DestroyLibrary();
  printf("done\n");
  return 0;
}
