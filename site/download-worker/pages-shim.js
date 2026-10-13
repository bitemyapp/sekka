// The loader Pages runs as _worker.js/index.js. It instantiates the download
// worker's Wasm (src/lib.rs, where everything happens) and gives Pages its
// fetch handler. worker-build fills in the $ placeholders and bundles it
// (CUSTOM_SHIM in nix/site.nix): its own loaders export an entrypoint
// class, while Pages' advanced mode calls fetch on a plain object.
import * as imports from "./index_bg.js";
import wasmModule from "./index.wasm";
$SNIPPET_JS_IMPORTS

const instance = new WebAssembly.Instance(wasmModule, {
  "./index_bg.js": imports,
  $SNIPPET_WASM_IMPORTS
});
imports.__wbg_set_wasm(instance.exports);
instance.exports.__wbindgen_start?.();

export default {
  fetch: (request, env, ctx) => imports.fetch(request, env, ctx),
};
