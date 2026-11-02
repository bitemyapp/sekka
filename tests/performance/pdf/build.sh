#!/usr/bin/env bash
# Builds every engine's benchmark program into $OUT (default
# $WORK/bin, by default $TMPDIR/aizome-perf/pdf/bin). Run inside shell.nix:
#   nix-shell tests/performance/pdf/shell.nix --run tests/performance/pdf/build.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-${WORK:-${TMPDIR:-/tmp}/aizome-perf/pdf}/bin}
TARGET=${TARGET:-${WORK:-${TMPDIR:-/tmp}/aizome-perf/pdf}/target}
mkdir -p "$OUT"
CFLAGS="-O2 -g -Wall"
cc $CFLAGS -I"$PDFIUM_INCLUDE" "$here/c/pdfium.c" -o "$OUT/pdfbench-pdfium" \
  -L"$PDFIUM_LIB" -Wl,-rpath,"$PDFIUM_LIB" -lpdfium -lm
cc $CFLAGS -I"$PDFIUM_INCLUDE" "$here/c/pdfium_pixel.c" -o "$OUT/pdfbench-pdfium-pixel" \
  -L"$PDFIUM_LIB" -Wl,-rpath,"$PDFIUM_LIB" -lpdfium -lm
cc $CFLAGS "$here/c/null.c" -o "$OUT/pdfbench-null"
cc $CFLAGS "$here/c/mupdf.c" -o "$OUT/pdfbench-mupdf" $(pkg-config --cflags --libs mupdf) -lm
c++ $CFLAGS "$here/c/poppler_splash.cc" -o "$OUT/pdfbench-poppler-splash" $(pkg-config --cflags --libs poppler-cpp)
cc $CFLAGS "$here/c/poppler_cairo.c" -o "$OUT/pdfbench-poppler-cairo" $(pkg-config --cflags --libs poppler-glib cairo)
for v in fork upstream; do
  cargo build --release --locked --manifest-path "$here/hayro/$v/Cargo.toml" --target-dir "$TARGET-$v" -j 8 2>&1 | tail -n 3
  cp "$TARGET-$v/release/pdfbench-hayro-$v" "$OUT/"
done
# For PDFium's pixel tests (conformance/pdfium_pixel_tests.py).
cp "$TARGET-fork/release/pdfbench-hayro-fork-pixel" "$TARGET-upstream/release/pdfbench-hayro-upstream-pixel" "$OUT/"
ls -la "$OUT"
