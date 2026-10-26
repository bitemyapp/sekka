# Changes something real in every pane of Settings (Settei), the way a
# person would, in a disposable VM: it opens a pane, clicks and types
# through the compositor (`aizomectl input`), checks the change reached the
# system (settings.toml, dconf, NetworkManager, PipeWire, BlueZ, CUPS,
# AccountsService, the compositor), then quits Settings, opens it again
# and checks the pane shows the new state.
#
# Settings writes where its controls are (AIZOME_KIT_INSPECT, the kit's
# inspection files), so the test finds a control by its label or its row
# as a person finds it by eye, and clicks its middle.
#
# The VM has what the panes need to have something to change: two virtio
# outputs, PipeWire with three null sinks, two simulated Wi-Fi radios
# (mac80211_hwsim) with hostapd serving a WPA2 network on one, a BlueZ
# virtual controller (btvirt over hci_vhci), CUPS with a driverless IPP
# printer announcing itself over mDNS, and power-profiles-daemon.
#
#     nix build .#checks.x86_64-linux.settings -L
#     nix build .#checks.x86_64-linux.settings.driverInteractive && ./result/bin/nixos-test-driver
{
  pkgs,
  self,
  # The Aizome package under test (the flake's by default).
  package ? null,
}:
let
  lib = pkgs.lib;
  # A test value: the passphrase of the simulated access point.
  wifiPassphrase = "aizome-wifi-test-7";
  # BlueZ's virtual controller (btvirt), which nixpkgs' BlueZ doesn't
  # install: BlueZ built with its testing tools, keeping only btvirt.
  btvirt =
    let
      bluez = pkgs.bluez.overrideAttrs (old: {
        pname = "bluez-testing";
        configureFlags = old.configureFlags ++ [ "--enable-testing" ];
        doCheck = false;
        postInstall = old.postInstall + ''
          install -Dm755 emulator/btvirt $out/bin/btvirt
        '';
      });
    in
    pkgs.runCommand "btvirt" { } "install -Dm755 ${bluez}/bin/btvirt $out/bin/btvirt";
in
pkgs.testers.runNixOSTest {
  name = "aizome-settings";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      programs.aizome.package = lib.mkIf (package != null) package;
      time.timeZone = "Europe/Paris";
      networking.hostName = "aizome-vm";
      # Date & Time switches automatic time on and off (the VM profile
      # turns timesyncd off).
      services.timesyncd.enable = lib.mkForce true;

      users.users.alice = {
        isNormalUser = true;
        description = "Alice Liddell";
        password = "aizome";
        extraGroups = [
          # An administrator, as the person setting up their computer is:
          # not in networkmanager or lpadmin, so the module must let wheel
          # change what Settings changes.
          "wheel"
          "video"
        ];
        uid = 1000;
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
          x = 1440;
          y = 900;
        };
        # Two outputs on one virtio GPU. QEMU reports only the first as
        # connected until a viewer shows the second, so the kernel is told
        # the second is there (video=…e).
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,max_outputs=2,xres=1440,yres=900"
          # A plain PS/2 mouse (relative, so pointer speed applies to it)
          # rather than VMware's absolute one.
          "-machine vmport=off"
        ];
      };
      boot.kernelParams = [ "video=Virtual-2:1920x1080e" ];

      # Every kit app (Settings, the shell) writes where its controls are,
      # for the script to find them as a person finds them by eye.
      systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/inspect";
      systemd.user.tmpfiles.rules = [ "d %t/inspect 0700 - - -" ];

      # Wi-Fi: two simulated radios; hostapd serves a WPA2 network on
      # wlan0 (with dnsmasq for addresses), NetworkManager has wlan1.
      boot.kernelModules = [
        "mac80211_hwsim"
        "hci_vhci"
      ];
      services.hostapd = {
        enable = true;
        radios.wlan0 = {
          band = "2g";
          channel = 6;
          countryCode = "US";
          networks.wlan0 = {
            ssid = "Aizome Test AP";
            authentication = {
              mode = "wpa2-sha1";
              wpaPassword = wifiPassphrase;
            };
            bssid = "02:00:00:00:00:00";
          };
        };
      };
      networking.networkmanager.unmanaged = [ "wlan0" ];
      # The access point's side: its DHCP requests come in through the
      # firewall.
      networking.firewall.trustedInterfaces = [ "wlan0" ];
      # NetworkManager drives wpa_supplicant over D-Bus; the VM profile
      # turns it off.
      networking.wireless.enable = lib.mkOverride 0 true;
      systemd.services.aizome-test-ap-addresses = {
        description = "Addresses for the simulated access point's clients";
        after = [ "hostapd.service" ];
        requires = [ "hostapd.service" ];
        wantedBy = [ "multi-user.target" ];
        path = [
          pkgs.iproute2
          pkgs.dnsmasq
        ];
        script = ''
          ip addr replace 10.77.0.1/24 dev wlan0
          exec dnsmasq --keep-in-foreground --interface=wlan0 --bind-interfaces --except-interface=lo \
            --port=0 --dhcp-authoritative --dhcp-range=10.77.0.10,10.77.0.99,1h --leasefile-ro
        '';
      };

      # Bluetooth: a virtual controller.
      systemd.services.aizome-test-btvirt = {
        description = "A virtual Bluetooth controller";
        before = [ "bluetooth.service" ];
        wantedBy = [ "multi-user.target" ];
        serviceConfig.ExecStart = "${btvirt}/bin/btvirt -l2";
      };

      # Sound: three outputs that go nowhere.
      services.pipewire.extraConfig.pipewire."90-aizome-test-sinks" = {
        "context.objects" =
          map
            (s: {
              factory = "adapter";
              args = {
                "factory.name" = "support.null-audio-sink";
                "node.name" = s.name;
                "node.description" = s.description;
                "media.class" = "Audio/Sink";
                "audio.position" = "FL,FR";
                "monitor.channel-volumes" = true;
              };
            })
            [
              {
                name = "aizome-speakers";
                description = "Studio Speakers";
              }
              {
                name = "aizome-headphones";
                description = "Desk Headphones";
              }
              {
                name = "aizome-hdmi";
                description = "Living Room TV";
              }
            ];
      };

      # Printers: a driverless IPP printer that announces itself (Avahi
      # lets programs announce services only when asked to), with a
      # keychain for the TLS that CUPS asks for.
      services.avahi.publish = {
        enable = true;
        userServices = true;
      };
      systemd.services.aizome-test-printer = {
        description = "A driverless IPP printer";
        after = [
          "avahi-daemon.service"
          "network.target"
        ];
        requires = [ "avahi-daemon.service" ];
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          # Avahi may not answer yet at boot.
          Restart = "on-failure";
          RestartSec = 2;
          ExecStart = "${pkgs.cups}/bin/ippeveprinter -K /var/lib/aizome-test-printer -p 8631 -r _print -M Aizome -m 'Test Printer' -f application/pdf,image/pwg-raster,image/urf 'Aizome Test Printer'";
          DynamicUser = true;
          StateDirectory = "aizome-test-printer";
          WorkingDirectory = "/var/lib/aizome-test-printer";
        };
      };

      environment.systemPackages = [
        pkgs.jq
        pkgs.wireplumber
        pkgs.pulseaudio
        pkgs.libinput
        pkgs.bluez
        pkgs.cups
        pkgs.dconf
        pkgs.glib
        pkgs.python3
        # setfacl: /dev/rfkill without uaccess, for airplane mode without rfkill.
        pkgs.acl
        # A second text editor and a second terminal for Default Apps.
        (pkgs.makeDesktopItem {
          name = "aizome-test-editor";
          desktopName = "Plain Editor";
          exec = "cat %F";
          mimeTypes = [ "text/plain" ];
        })
        (pkgs.makeDesktopItem {
          name = "aizome-test-terminal";
          desktopName = "Test Terminal";
          exec = "foot --app-id=aizome-test-terminal";
          categories = [
            "System"
            "TerminalEmulator"
          ];
        })
      ];
    };

  enableOCR = false;

  # The script is its own file, so the driver can run it from a checkout
  # (`--test-script`); the passphrase comes from here.
  testScript = ''
    WIFI_PASSPHRASE = ${builtins.toJSON wifiPassphrase}
  ''
  + builtins.readFile ./vm-settings-test.py;
}
