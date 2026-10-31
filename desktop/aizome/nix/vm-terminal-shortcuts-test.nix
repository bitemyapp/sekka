# Component regression: real clients and keyboard/pointer input.
{
  pkgs,
  self,
  package ? null,
}:
let
  python = pkgs.python3.withPackages (ps: [ ps.pygobject3 ]);
in
pkgs.testers.runNixOSTest {
  name = "aizome-terminal-shortcuts";
  globalTimeout = 360;
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkIf (package != null) package;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
    };
    # Upgraded from before Hotaru: foot was chosen in Settei, which wrote it
    # to the terminal list and, for ❖Return, to the settings file. The
    # first session moves it to Hotaru.
    systemd.tmpfiles.settings."10-before-hotaru" =
      let
        alice = {
          user = "alice";
          group = "users";
        };
      in
      {
        "/home/alice/.config".d = alice // {
          mode = "0755";
        };
        "/home/alice/.config/aizome".d = alice // {
          mode = "0755";
        };
        "/home/alice/.config/aizome/settings.toml".f = alice // {
          mode = "0644";
          argument = ''[keys]\nterminal = ["foot"]\n'';
        };
        "/home/alice/.config/xdg-terminals.list".f = alice // {
          mode = "0644";
          argument = ''foot.desktop\n'';
        };
      };
    services.greetd = {
      enable = true;
      settings.initial_session = {
        user = "alice";
        command = "/run/current-system/sw/bin/aizome-session";
      };
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    environment.systemPackages = [
      # Explicit compatibility fixture; Foot is not a desktop default.
      pkgs.foot
      python
      pkgs.wl-clipboard
    ];
    environment.sessionVariables.GI_TYPELIB_PATH = lib.makeSearchPath "lib/girepository-1.0" (
      map lib.getLib [
        pkgs.gtk3
        pkgs.pango
        pkgs.gdk-pixbuf
        pkgs.atk
        pkgs.harfbuzz
        pkgs.glib
        pkgs.gobject-introspection
      ]
    );
    environment.etc."shortcut-probe.py".text = ''
      import gi
      gi.require_version("Gtk", "3.0")
      from gi.repository import Gtk, Gdk
      window = Gtk.Window(title="GTK command translation")
      window.set_default_size(420, 200)
      window.add(Gtk.Label(label="GTK keeps Ctrl+W and Ctrl+Q"))
      def key(widget, event):
          with open("/tmp/gtk-keys", "a") as out:
              out.write("%s ctrl=%s shift=%s super=%s\n" % (
                  Gdk.keyval_name(event.keyval),
                  bool(event.state & Gdk.ModifierType.CONTROL_MASK),
                  bool(event.state & Gdk.ModifierType.SHIFT_MASK),
                  bool(event.state & Gdk.ModifierType.SUPER_MASK)))
          return False
      window.connect("key-press-event", key)
      window.connect("delete-event", lambda *args: Gtk.main_quit())
      window.show_all()
      Gtk.main()
    '';
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
  };
  testScript = builtins.readFile ./vm-terminal-shortcuts-test.py;
}
