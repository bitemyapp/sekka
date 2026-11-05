# SPDX-License-Identifier: MIT OR Apache-2.0
# The machine the window's VM tests share: Aizome with alice logged in (her
# password is test-only, and she's an administrator, so polkit's
# auth_admin asks for it), Tasuke installed with a scripted model
# (fake-openai.py) standing in for llama-server as tasuke-llm.service, and
# kit apps reporting their controls under /run/user/1000/inspect, where
# the test finds what to click (vm-desktop.py).
{
  pkgs,
  tasuke,
  tasukeModule,
  aizomeModule,
}:
let
  fakeLlamaServer = pkgs.writeShellScriptBin "llama-server" ''
    exec ${pkgs.python3}/bin/python3 -I ${./fake-openai.py} 8737 "$XDG_RUNTIME_DIR/tasuke/llama-requests.jsonl"
  '';
in
{
  imports = [
    aizomeModule
    tasukeModule
  ];
  programs.aizome.enable = true;
  programs.tasuke = {
    enable = true;
    package = tasuke;
    llamaCpp = fakeLlamaServer;
  };
  users.users.alice = {
    isNormalUser = true;
    uid = 1000;
    password = "test-only";
    extraGroups = [ "wheel" ];
  };
  services.greetd = {
    enable = true;
    settings.initial_session = {
      user = "alice";
      command = "/run/current-system/sw/bin/aizome-session";
    };
    settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
  };
  systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/inspect";
  systemd.user.tmpfiles.rules = [ "d %t/inspect 0700 - - -" ];
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
}
