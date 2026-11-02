// pdf.js (pdfjs-dist, legacy build for Node) drawing into @napi-rs/canvas
// (Skia), as the benchmark programs in ../c and ../hayro do: open FILE, draw
// its first pages (5, or PDFBENCH_PAGES) at PDFBENCH_SCALE (2) on white,
// timing each page from getPage to its finished pixels, and print
//   open <ms> <pages> | error <why>;  page <index> <ms> <width> <height>;  done
// With OUT_DIR, pages are written as <index>.ppm outside the timed part.
// pdf.js's worker runs as its "fake worker" in this thread, so parsing and
// drawing share one thread, like the other engines.
//
//   node bench.mjs FILE.pdf [OUT_DIR]   (NODE_PATH: a node_modules with
//   pdfjs-dist and @napi-rs/canvas; see package.json)
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";

const require = createRequire(join(process.env.PDFJS_MODULES ?? process.cwd(), "x.js"));
const pdfjsDir = dirname(require.resolve("pdfjs-dist/package.json"));
const pdfjs = await import(pathToFileURL(join(pdfjsDir, "legacy/build/pdf.mjs")).href);
const { createCanvas } = require("@napi-rs/canvas");

const [file, outDir] = process.argv.slice(2);
const maxPages = process.env.PDFBENCH_PAGES === "all" ? Infinity : Number(process.env.PDFBENCH_PAGES ?? 5) || 5;
const scale = Number(process.env.PDFBENCH_SCALE ?? 2) || 2;
const out = (s) => process.stdout.write(s + "\n");

let t = performance.now();
let doc;
try {
  const data = new Uint8Array(readFileSync(file));
  doc = await pdfjs.getDocument({
    data,
    standardFontDataUrl: join(pdfjsDir, "standard_fonts") + "/",
    cMapUrl: join(pdfjsDir, "cmaps") + "/",
    cMapPacked: true,
    wasmUrl: join(pdfjsDir, "wasm") + "/",
    iccUrl: join(pdfjsDir, "iccs") + "/",
    verbosity: 0,
  }).promise;
} catch (e) {
  out(`error\t${String(e?.name ?? e).replace(/\s+/g, " ")}`);
  process.exit(0);
}
out(`open\t${(performance.now() - t).toFixed(3)}\t${doc.numPages}`);
for (let i = 0; i < doc.numPages && i < maxPages; i++) {
  t = performance.now();
  try {
    const page = await doc.getPage(i + 1);
    const viewport = page.getViewport({ scale });
    const w = Math.floor(viewport.width), h = Math.floor(viewport.height);
    const canvas = createCanvas(Math.max(w, 1), Math.max(h, 1));
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "white";
    ctx.fillRect(0, 0, w, h);
    await page.render({ canvas, canvasContext: ctx, viewport, background: "white" }).promise;
    const rgba = ctx.getImageData(0, 0, w, h).data;
    const took = performance.now() - t;
    out(`page\t${i}\t${took.toFixed(3)}\t${w}\t${h}`);
    if (outDir) {
      const head = Buffer.from(`P6\n${w} ${h}\n255\n`);
      const rgb = Buffer.alloc(w * h * 3);
      for (let p = 0, q = 0; p < rgba.length; p += 4, q += 3) {
        rgb[q] = rgba[p]; rgb[q + 1] = rgba[p + 1]; rgb[q + 2] = rgba[p + 2];
      }
      writeFileSync(join(outDir, `${i}.ppm`), Buffer.concat([head, rgb]));
    }
    page.cleanup();
  } catch (e) {
    out(`fail\t${i}\t${String(e?.message ?? e).replace(/\s+/g, " ").slice(0, 200)}`);
  }
}
out("done");
process.exit(0);
