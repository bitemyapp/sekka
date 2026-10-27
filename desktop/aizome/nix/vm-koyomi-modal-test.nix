# A real Koyomi window: confirmation sheets must own input above its editor.
{ pkgs, self }:
pkgs.testers.runNixOSTest {
  name = "aizome-koyomi-modal";
  globalTimeout = 300;
  nodes.machine = {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    time.timeZone = "America/Chicago";
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
    # Disposable calendar data, not a sample/headless application mode.
    environment.etc."koyomi-modal-fixture.ics".text = ''
      BEGIN:VCALENDAR
      VERSION:2.0
      X-WR-CALNAME:Modal regression
      BEGIN:VEVENT
      UID:modal-regression@aizome.test
      DTSTAMP:20261021T120000Z
      DTSTART;VALUE=DATE:20261022
      DTEND;VALUE=DATE:20261023
      RRULE:FREQ=DAILY;COUNT=3
      SUMMARY:Three day test
      END:VEVENT
      END:VCALENDAR
    '';
  };
  testScript = builtins.readFile ./vm-koyomi-modal-test.py;
}
