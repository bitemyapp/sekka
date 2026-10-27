# Real NetworkManager, wpa_supplicant, keyring, VPN plugin and native
# dialogs, on virtual radios and a VPN server entirely inside this VM.
{
  pkgs,
  self,
  package ? null,
}:
let
  inherit (pkgs) lib;
  credentials = pkgs.writeShellScript "network-test-certificates" ''
    mkdir -p /run/network-test
    ${pkgs.openssl}/bin/openssl req -x509 -newkey rsa:2048 -nodes \
      -keyout /run/network-test/server.key -out /run/network-test/server.crt \
      -days 2 -subj /CN=auth.test \
      -addext subjectAltName=DNS:auth.test -addext extendedKeyUsage=serverAuth \
      -addext keyUsage=digitalSignature,keyEncipherment
    chmod 600 /run/network-test/server.key
    # NixOS's wpa_supplicant runs in a filesystem sandbox containing /etc.
    cp /run/network-test/server.crt /etc/network-test-ca.crt
  '';
  verifyVpn = pkgs.writeShellScript "verify-test-vpn-user" ''
    { IFS= read -r vpn_test_user; IFS= read -r vpn_test_password; } < "$1"
    [ "$vpn_test_user" = vpnuser ] && [ "$vpn_test_password" = vpn-test-password ]
  '';
  eapUsers = pkgs.writeText "test-eap-users" ''
    * PEAP
    "enterprise-user" MSCHAPV2 "enterprise-password" [2]
  '';
  ap =
    interface: ssid: extra:
    pkgs.writeText "${interface}-hostapd.conf" ''
      interface=${interface}
      driver=nl80211
      ssid=${ssid}
      hw_mode=g
      channel=6
      country_code=US
      ctrl_interface=/run/hostapd
      ${extra}
    '';
  wpa2 = ap "wlan0" "Aizome WPA2" ''
    wpa=2
    wpa_key_mgmt=WPA-PSK
    rsn_pairwise=CCMP
    wpa_passphrase=wifi-test-password
  '';
  wpa3 = ap "wlan2" "Aizome WPA3" ''
    wpa=2
    wpa_key_mgmt=SAE
    rsn_pairwise=CCMP
    ieee80211w=2
    sae_password=wifi-test-password
  '';
  enterprise = ap "wlan3" "Aizome Enterprise" ''
    wpa=2
    wpa_key_mgmt=WPA-EAP
    rsn_pairwise=CCMP
    ieee8021x=1
    eap_server=1
    eap_user_file=${eapUsers}
    ca_cert=/run/network-test/server.crt
    server_cert=/run/network-test/server.crt
    private_key=/run/network-test/server.key
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-network-auth";
  nodes.machine = { ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkIf (package != null) package;
    users.users.alice = {
      isNormalUser = true;
      password = "aizome";
      uid = 1000;
      extraGroups = [
        "wheel"
        "video"
        # The driver's su sessions have no seat. Allow their nmcli
        # activation requests; they still never provide credentials.
        "networkmanager"
      ];
    };
    services.greetd = {
      enable = true;
      useTextGreeter = true;
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      resolution = {
        x = 1440;
        y = 900;
      };
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1440,yres=900"
      ];
    };
    systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/inspect";
    systemd.user.tmpfiles.rules = [ "d %t/inspect 0700 - - -" ];
    # Both startup paths must be blocked in Aizome and usable elsewhere.
    programs.nm-applet.enable = true;
    environment.etc."xdg/autostart/nm-applet.desktop".source =
      lib.mkDefault "${pkgs.networkmanagerapplet}/etc/xdg/autostart/nm-applet.desktop";
    networking.networkmanager = {
      unmanaged = [
        "wlan0"
        "wlan2"
        "wlan3"
      ];
      plugins = [ pkgs.networkmanager-openvpn ];
    };
    networking.wireless.enable = lib.mkOverride 0 true;
    networking.firewall.trustedInterfaces = [
      "wlan0"
      "wlan2"
      "wlan3"
      "tun-server"
    ];
    boot.kernelModules = [
      "mac80211_hwsim"
      "tun"
    ];
    boot.extraModprobeConfig = "options mac80211_hwsim radios=4";
    systemd.services.network-test-certificates = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = credentials;
      };
    };
    systemd.services.network-test-ap = {
      requires = [ "network-test-certificates.service" ];
      after = [
        "network-test-certificates.service"
        "sys-subsystem-net-devices-wlan0.device"
      ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.hostapd}/bin/hostapd ${wpa2} ${wpa3} ${enterprise}";
    };
    systemd.services.network-test-dhcp = {
      requires = [ "network-test-ap.service" ];
      after = [ "network-test-ap.service" ];
      wantedBy = [ "multi-user.target" ];
      path = [
        pkgs.iproute2
        pkgs.dnsmasq
      ];
      script = ''
        ip addr replace 10.77.0.1/24 dev wlan0
        ip addr replace 10.78.0.1/24 dev wlan2
        ip addr replace 10.79.0.1/24 dev wlan3
        exec dnsmasq --keep-in-foreground --bind-interfaces --port=0 \
          --interface=wlan0 --interface=wlan2 --interface=wlan3 --except-interface=lo \
          --dhcp-range=10.77.0.10,10.77.0.99,1h \
          --dhcp-range=10.78.0.10,10.78.0.99,1h \
          --dhcp-range=10.79.0.10,10.79.0.99,1h --leasefile-ro
      '';
    };
    systemd.services.network-test-vpn = {
      requires = [ "network-test-certificates.service" ];
      after = [ "network-test-certificates.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = lib.concatStringsSep " " [
        "${pkgs.openvpn}/bin/openvpn --dev tun-server --local 127.0.0.1 --port 1194"
        "--proto udp --server 10.88.0.0 255.255.255.0 --topology subnet"
        "--ca /run/network-test/server.crt --cert /run/network-test/server.crt"
        "--key /run/network-test/server.key --dh none --verify-client-cert none"
        "--username-as-common-name --auth-user-pass-verify ${verifyVpn} via-file"
        "--script-security 2 --data-ciphers AES-256-GCM --verb 3"
      ];
    };
    environment.systemPackages = [
      pkgs.jq
      pkgs.libsecret
      pkgs.gnome-keyring
      pkgs.python3
      pkgs.glib
      pkgs.networkmanagerapplet
      pkgs.openvpn
      pkgs.hostapd
    ];
  };
  enableOCR = false;
  testScript = builtins.readFile ./vm-network-auth-test.py;
}
