# Real packaged Shiori in the Aizome session: it opens a document with its
# PDFium worker (found through the package's run path), finds text through
# that worker, carries on when the worker is killed, and is the session's
# PDF reader.
{ pkgs, self }:
let
  # Three letter pages of Helvetica text, "needle" on two of them.
  document =
    pkgs.runCommand "shiori-sample.pdf" { nativeBuildInputs = [ pkgs.python3 ]; }
      ''
        python - "$out" <<'PY'
        import sys
        lines = [
            ["Shiori reads documents.", "The first page has a needle."],
            ["Nothing to find here."],
            ["The last page has a needle too."],
        ]
        n = len(lines)
        objs = ["<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [%s] /Count %d >>" % (" ".join("%d 0 R" % (4 + 2 * i) for i in range(n)), n),
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
        for i, page in enumerate(lines):
            content = "".join("BT /F1 18 Tf 72 %d Td (%s) Tj ET\n" % (720 - 30 * j, t) for j, t in enumerate(page))
            objs.append("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                        "/Resources << /Font << /F1 3 0 R >> >> /Contents %d 0 R >>" % (5 + 2 * i))
            objs.append("<< /Length %d >>\nstream\n%sendstream" % (len(content), content))
        out = b"%PDF-1.7\n"
        offsets = []
        for i, o in enumerate(objs):
            offsets.append(len(out))
            out += b"%d 0 obj\n%s\nendobj\n" % (i + 1, o.encode())
        xref = len(out)
        out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
        out += b"".join(b"%010d 00000 n \n" % o for o in offsets)
        out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
        open(sys.argv[1], "wb").write(out)
        PY
      '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-shiori";
  globalTimeout = 300;
  nodes.machine = {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
    };
    services.greetd = {
      enable = true;
      settings.initial_session = {
        user = "alice";
        command = "/run/current-system/sw/bin/aizome-session";
      };
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    environment.systemPackages = [ pkgs.xdg-utils ];
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      resolution = {
        x = 1280;
        y = 800;
      };
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
    environment.etc."shiori-sample.pdf".source = document;
  };
  testScript = builtins.readFile ./vm-shiori-test.py;
}
