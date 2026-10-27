# Koyomi must not retain a completed desktop when the user manager lingers.
{ pkgs, self }:
pkgs.testers.runNixOSTest {
  name = "aizome-koyomi-session";
  globalTimeout = 600;
  nodes.machine = {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    services.xserver.enable = true;
    services.xserver.desktopManager.xfce.enable = true;
    services.desktopManager.gnome.enable = true;
    services.displayManager.gdm.enable = true;
    services.displayManager.autoLogin = {
      enable = true;
      user = "demo";
    };
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
      linger = true;
    };
    # Select the real GDM session through its normal AccountsService state.
    # Leave defaultSession unset so restarting GDM retains the selection.
    system.activationScripts.koyomiTestSession.text = ''
      mkdir -p /var/lib/AccountsService/users
      if [ ! -e /var/lib/AccountsService/users/demo ]; then
        cat > /var/lib/AccountsService/users/demo <<'EOF'
      [User]
      Session=xfce
      XSession=xfce
      SessionType=x11
      SystemAccount=false
      EOF
      fi
    '';
    virtualisation = {
      memorySize = 6144;
      cores = 4;
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
  };
  testScript = builtins.readFile ./vm-koyomi-session-test.py;
}
