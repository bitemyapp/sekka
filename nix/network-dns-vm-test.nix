# Actual Network controls and NetworkManager profile transactions. Component
# fixture only: autologin, software GPU, private NAT and a saved test WPA profile.
{
  pkgs,
  desktopsModule,
  network,
}:
let
  profileTests = network.overrideAttrs (old: {
    pname = "tatami-network-profile-tests";
    postInstall = old.postInstall + ''
      mapfile -t tests < <(find target -type f -name 'tatami_network-*' -executable \
        -exec grep -al 'profile::tests::network_profile_transactions' {} \;)
      test "''${#tests[@]}" = 1
      install -Dm755 "''${tests[0]}" $out/libexec/network-profile-tests
    '';
  });
  python = pkgs.python3.withPackages (p: [ p.dbus-python ]);
in
pkgs.testers.runNixOSTest {
  name = "tatami-network-dns";
  globalTimeout = 900;
  node.pkgsReadOnly = false;
  nodes.machine = { ... }: {
    imports = [ desktopsModule ];
    calamares.desktops = [ "tatami" ];
    calamares.defaultDesktop = "tatami";
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      password = "demo";
      extraGroups = [
        "wheel"
        "networkmanager"
      ];
    };
    services.displayManager.autoLogin = {
      enable = true;
      user = "demo";
    };
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
    hardware.graphics.enable = true;
    networking.networkmanager.enable = true;
    programs.ydotool.enable = true;
    environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    environment.systemPackages = [ python ];
  };
  testScript = ''
    import csv, io, json, pathlib, shlex, subprocess, time, textwrap

    def user(command):
        return machine.succeed("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} sh -c " + shlex.quote(command))
    def wait_for(predicate, seconds=20):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate(): return
            time.sleep(0.2)
        machine.screenshot("failure")
        raise AssertionError("Condition timed out")
    def window():
        return next((c for c in json.loads(user("hyprctl -j clients")) if c["class"] == "org.tatami.Network"), None)
    def click(x, y):
        user("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        time.sleep(0.2)
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool click 0xC0")
    def bottom():
        c = window()
        user("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {c['at'][0]+c['size'][0]-35}, y = {c['at'][1]+c['size'][1]-80} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove --wheel -x 0 -y -60")
        time.sleep(1)
    custom = False
    selection = 0
    def combo(offset, label):
        global selection
        bottom()
        c = window()
        button_y = c["at"][1]+c["size"][1]-offset
        click(c["at"][0]+c["size"][0]-120, button_y)
        time.sleep(0.3)
        selection += 1
        capture = f"selector-{selection:02d}-{label}"
        machine.screenshot(capture)
        image = pathlib.Path(machine.out_dir) / (capture + ".png")
        rows = csv.DictReader(io.StringIO(subprocess.check_output(["${pkgs.tesseract}/bin/tesseract", str(image), "stdout", "--psm", "11", "tsv"], text=True)), delimiter="\t")
        matches = [(int(r["left"])+int(r["width"])/2, int(r["top"])+int(r["height"])/2) for r in rows
            if r["text"].strip().casefold() == label.casefold() and int(r["left"]) > c["at"][0]+c["size"][0]*0.65
            and abs(int(r["top"])+int(r["height"])/2-button_y) > 20]
        assert len(matches) == 1, (label, matches)
        click(*matches[0])
        time.sleep(2)
    def dns(index):
        global custom
        combo(225 if custom else 171, ["Automatic", "Cloudflare", "Google", "Custom"][index])
        custom = index == 3
    def ipv6(index):
        combo(51, ["Automatically", "Off"][index])
    def custom_dns(servers):
        if not custom: dns(3)
        bottom()
        c = window()
        click(c["at"][0]+c["size"][0]/2, c["at"][1]+c["size"][1]-171)
        machine.send_key("ctrl-a")
        machine.send_chars(servers)
        machine.send_key("ret")
        time.sleep(2)
    def field(name):
        return machine.succeed("nmcli --escape no -g " + shlex.quote(name) + " connection show uuid " + shlex.quote(uuid)).strip()
    def snapshot(name):
        print(name, machine.succeed("nmcli -f connection.id,connection.uuid,connection.metered,ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns,ipv4.ignore-auto-dns,ipv6.method,ipv6.dns,ipv6.ignore-auto-dns connection show uuid " + uuid))
        print(machine.succeed("ip -brief address show eth0; cat /etc/resolv.conf"))
        machine.screenshot(name)
    def open_details():
        global custom
        user("XDG_CONFIG_DIRS=/etc/xdg/tatami:/etc/xdg tatami launch tatami-network")
        wait_for(lambda: window() is not None)
        time.sleep(2)
        c = window()
        click(c["at"][0]+c["size"][0]/2, c["at"][1]+134)
        time.sleep(2)
        bottom()
        custom = False
    def close():
        machine.send_key("meta_l-w")
        wait_for(lambda: window() is None)
    def set_metadata(connection, values):
        setup = """
    import dbus
    bus = dbus.SystemBus()
    settings = dbus.Interface(bus.get_object('org.freedesktop.NetworkManager', '/org/freedesktop/NetworkManager/Settings'), 'org.freedesktop.NetworkManager.Settings')
    path = settings.GetConnectionByUuid(""" + repr(connection) + """)
    profile = dbus.Interface(bus.get_object('org.freedesktop.NetworkManager', path), 'org.freedesktop.NetworkManager.Settings.Connection')
    data = profile.GetSettings()
    old = data.get('user', {}).get('data', {})
    old.update(""" + repr(values) + """)
    data['user'] = dbus.Dictionary({'data': dbus.Dictionary(old, signature='ss', variant_level=1)}, signature='sv')
    profile.Update2(data, dbus.UInt32(1), dbus.Dictionary({}, signature='sv'))
    """
        machine.succeed("python3 -c " + shlex.quote(textwrap.dedent(setup)))

    machine.start()
    machine.wait_for_unit("ydotoold.service")
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    machine.wait_until_succeeds("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus systemctl --user is-active tatami-waybar.service", timeout=120)
    uuid = machine.succeed("nmcli -g GENERAL.CON-UUID device show eth0").strip()
    original_v4 = [field(k) for k in ["ipv4.method", "ipv4.addresses", "ipv4.gateway"]]
    print("PRODUCTION NETWORK", machine.succeed("readlink -f /run/current-system/sw/bin/tatami-network; sha256sum /run/current-system/sw/bin/tatami-network"))
    open_details()
    snapshot("initial")
    # The same script collects both original failures before asserting them.
    failures = []
    ipv6(1)
    assert field("ipv6.method") == "disabled"
    dns(1)
    assert field("ipv4.dns") == "1.1.1.1,1.0.0.1"
    ipv6(0)
    time.sleep(4)
    snapshot("cloudflare-off-auto")
    if field("ipv6.ignore-auto-dns") != "yes" or field("ipv6.dns") != "2606:4700:4700::1111,2606:4700:4700::1001":
        failures.append("Cloudflare policy lost when IPv6 re-enabled")
    if "nameserver fec0::3" in machine.succeed("cat /etc/resolv.conf"):
        failures.append("Network IPv6 DNS reappeared beside Cloudflare")
    dns(2)
    snapshot("google-before-off")
    assert field("ipv6.dns") == "2001:4860:4860::8888,2001:4860:4860::8844"
    ipv6(1)
    snapshot("google-auto-off")
    if field("ipv6.method") != "disabled":
        failures.append("Google DNS prevents IPv6 Off")
    assert not failures, failures
    assert field("ipv6.dns") == ""
    machine.succeed("test $(cat /proc/sys/net/ipv6/conf/eth0/disable_ipv6) = 1")
    machine.succeed("ping -4 -c 2 -W 2 10.0.2.2")
    assert [field(k) for k in ["ipv4.method", "ipv4.addresses", "ipv4.gateway"]] == original_v4

    # Choice changes while Off replace suspended addresses, not only IPv4.
    dns(1)
    close()
    open_details()
    ipv6(0)
    assert field("ipv6.dns") == "2606:4700:4700::1111,2606:4700:4700::1001"
    snapshot("cloudflare-restored-after-reopen")
    custom_dns("9.9.9.9 9.9.9.9 2620:00fe::fe 2620:fe::fe")
    assert field("ipv4.dns") == "9.9.9.9"
    assert field("ipv6.dns") == "2620:fe::fe"
    custom_dns("not-a-server")
    assert field("ipv4.dns") == "9.9.9.9" and field("ipv6.dns") == "2620:fe::fe"
    snapshot("invalid-custom-preserves-policy")
    custom_dns("9.9.9.9 2620:fe::fe")
    ipv6(1)
    snapshot("custom-off")
    close()
    # Read the actual persisted NM profile again, including its opaque metadata.
    machine.succeed("systemctl restart NetworkManager")
    machine.wait_until_succeeds("nmcli -g GENERAL.STATE device show eth0 | grep -q '^100'")
    open_details()
    custom = True
    snapshot("custom-off-after-daemon-restart")
    assert field("ipv6.method") == "disabled"
    ipv6(0)
    assert field("ipv6.dns") == "2620:fe::fe"
    snapshot("custom-auto-restored")
    for servers, v4, v6 in [("9.9.9.9", "9.9.9.9", ""), ("2620:fe::fe", "", "2620:fe::fe")]:
        custom_dns(servers)
        ipv6(1)
        assert field("ipv6.dns") == ""
        ipv6(0)
        assert field("ipv4.dns") == v4 and field("ipv6.dns") == v6
        assert field("ipv4.ignore-auto-dns") == "yes" and field("ipv6.ignore-auto-dns") == "yes"
        snapshot("custom-v4-only" if v4 else "custom-v6-only")
    # A corrupt or externally stale backup is refused before any mutation.
    ipv6(1)
    close()
    set_metadata(uuid, {"org.sekka.tatami.ipv6-dns": "v2|unknown"})
    open_details()
    custom = True
    ipv6(0)
    assert field("ipv6.method") == "disabled" and field("ipv6.dns") == ""
    snapshot("malformed-backup-refused")
    dns(0)
    ipv6(0)
    custom_dns("9.9.9.9 2620:fe::fe")
    ipv6(1)
    close()
    machine.succeed("nmcli connection modify uuid " + uuid + " ipv4.dns 8.8.8.8")
    open_details()
    ipv6(0)
    assert field("ipv6.method") == "disabled" and field("ipv4.dns") == "8.8.8.8"
    snapshot("externally-changed-backup-refused")
    dns(0)
    assert field("ipv4.dns") == "" and field("ipv6.dns") == ""
    ipv6(0)
    assert field("ipv4.ignore-auto-dns") == "no" and field("ipv6.ignore-auto-dns") == "no"
    time.sleep(4)
    snapshot("automatic-restored")
    machine.succeed("ping -4 -c 2 -W 2 10.0.2.2")
    assert [field(k) for k in ["ipv4.method", "ipv4.addresses", "ipv4.gateway"]] == original_v4
    close()

    # Inactive disposable WPA profile: no host radio, no actual association.
    machine.succeed("nmcli connection add type wifi con-name 'Tatami DNS fixture' ifname fixture0 ssid Fixture connection.autoconnect no ipv4.method auto ipv6.method auto wifi-sec.key-mgmt wpa-psk wifi-sec.psk disposable-fixture-password")
    test_uuid = machine.succeed("nmcli -g connection.uuid connection show 'Tatami DNS fixture'").strip()
    set_metadata(test_uuid, {"fixture.unrelated": "keep, this = value"})
    print(machine.succeed("TATAMI_NETWORK_VM_TEST_UUID=" + test_uuid + " ${profileTests}/libexec/network-profile-tests --ignored --nocapture network_profile_transactions"))
    machine.succeed("nmcli connection delete uuid " + test_uuid)
    print(machine.succeed("systemctl --user --machine=demo@ --failed --no-pager; coredumpctl list --no-pager || true"))
  '';
}
