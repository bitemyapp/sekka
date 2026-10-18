{ upstream }:
upstream.override {
  chatgpt-unwrapped = upstream.unwrapped.overrideAttrs (old: {
    postInstall = (old.postInstall or "") + ''
      python3 ${./patch-asar.py} "$out/lib/chatgpt/resources/app.asar"
      cp ${./copy-plugin.cjs} "$out/lib/chatgpt/resources/sekka-copy-plugin.cjs"
    '';
  });
}
