# The machine the agent evaluations break and the agents repair or use:
# a Sekka system with Aizome its only desktop, as Genkan installs one
# (the desktops module, Sekka's tuning), logged in automatically as alice,
# with a few ordinary services to break and a small "LAN" to reach.
#
# Nothing here is broken. Each scenario (harness/scenarios.py) breaks it
# at run time, after the desktop is up, so one system serves them all.
{
  desktopsModule,
  # A key pair for the SSH scenario: a test fixture, made at build time.
  sshKeys,
  # oh-my-pi, for the `--agent omp` condition (the flake's pinned input).
  ompPackage ? null,
  # The screen (the harness's computer-use points assume 1440×900).
  screen ? {
    x = 1440;
    y = 900;
  },
}:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  python = pkgs.python3;
  # The "LAN": a network namespace standing in for the router and an
  # update server, reachable over a veth pair. dnsmasq answers for
  # sekka.test only; a web server says it is up.
  lanWww = pkgs.writeTextDir "index.html" "sekka update server: ok\n";
  notesWww = pkgs.writeTextDir "index.html" "<h1>Alice's notes</h1>\n<p>Dentist on Thursday.</p>\n";
  # Runs a command as alice inside her graphical session.
  asAlice = pkgs.writeShellScriptBin "as-alice" ''
    # Runs a command as alice, in her graphical session's environment.
    #   as-alice systemctl --user status
    #   as-alice aizomectl --json windows
    wayland=$(ls /run/user/1000 2>/dev/null | grep -m1 '^wayland-[0-9]*$' || true)
    exec ${pkgs.util-linux}/bin/runuser -u alice -- env \
      XDG_RUNTIME_DIR=/run/user/1000 \
      DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
      WAYLAND_DISPLAY="$wayland" \
      AIZOME_SOCKET="/run/user/1000/aizome.$wayland.sock" \
      DCONF_PROFILE=aizome \
      HOME=/home/alice USER=alice LOGNAME=alice \
      PATH=/run/wrappers/bin:/home/alice/.nix-profile/bin:/etc/profiles/per-user/alice/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin \
      "$@"
  '';
  backup = pkgs.writeShellScript "sekka-backup" ''
    set -eu
    dest=''${BACKUP_DEST:?BACKUP_DEST is not set}
    if [ ! -d "$dest" ]; then
      echo "backup destination $dest does not exist" >&2
      exit 1
    fi
    name="$dest/alice-documents-$(date +%Y%m%d-%H%M%S).tar.gz"
    tar -czf "$name.part" -C /home/alice Documents
    mv "$name.part" "$name"
    echo "backup written to $name"
  '';
in
{
  imports = [ desktopsModule ];

  calamares.desktops = [ "aizome" ];
  calamares.defaultDesktop = "aizome";
  calamares.tuning.enable = true;
  # The test driver reads the console, which tuning quietens.
  boot.consoleLogLevel = lib.mkForce 7;
  networking.hostName = "studio";

  services.displayManager.autoLogin = {
    enable = true;
    user = "alice";
  };
  users.users.alice = {
    isNormalUser = true;
    uid = 1000;
    description = "Alice Liddell";
    password = "aizome";
    extraGroups = [
      "wheel"
      "networkmanager"
      "video"
    ];
  };

  virtualisation = {
    memorySize = 4096;
    cores = 4;
    diskSize = 4096;
    resolution = screen;
    qemu.options = [
      "-vga none"
      "-device virtio-gpu-pci,xres=${toString screen.x},yres=${toString screen.y}"
    ];
    # /srv/data: a small disk of its own, so filling it fills only it.
    emptyDiskImages = [ 64 ];
  };
  hardware.graphics.enable = true;

  # The store is the host's, shared over virtiofs with --cache=always:
  # virtiofsd keeps a file handle for every inode the guest has cached, so
  # a store-wide `find` (the host's store is far bigger than this
  # machine's) exhausted its handles and broke every later library load.
  # Reclaiming dentries and inodes makes the guest forget them.
  systemd.services.virtiofs-inode-release = {
    description = "Release cached /nix/store inodes (a test VM's shared store)";
    wantedBy = [ "multi-user.target" ];
    serviceConfig.ExecStart = pkgs.writeShellScript "virtiofs-inode-release" ''
      while sleep 2; do echo 2 > /proc/sys/vm/drop_caches; done
    '';
  };

  # Every kit window (apps and the shell) writes where its controls are:
  # the computer-use scenarios offer that as a text observation.
  systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/inspect";
  systemd.user.tmpfiles.rules = [ "d %t/inspect 0700 - - -" ];

  environment.systemPackages = lib.optional (ompPackage != null) ompPackage ++ [
    asAlice
    python
    pkgs.curl
    pkgs.dnsutils
    pkgs.file
    pkgs.jq
    pkgs.lsof
    pkgs.nftables
    pkgs.psmisc
    pkgs.strace
    pkgs.tree
    pkgs.wtype
    pkgs.e2fsprogs
  ];

  # Sound: one output that goes nowhere, and two more to choose from.
  services.pipewire.extraConfig.pipewire."90-eval-sinks" = {
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
          };
        })
        [
          {
            name = "eval-speakers";
            description = "Studio Speakers";
          }
          {
            name = "eval-headphones";
            description = "Desk Headphones";
          }
          {
            name = "eval-hdmi";
            description = "Living Room TV";
          }
        ];
  };

  # The LAN: 10.88.0.1 here, 10.88.0.2 the router/update server.
  networking.nameservers = [ "10.88.0.2" ];
  networking.networkmanager.unmanaged = [ "lan0" ];
  # Only the LAN's DNS: NetworkManager would add QEMU's user network's
  # (10.0.2.3), which answers nothing here.
  networking.networkmanager.dns = "none";
  systemd.services.lan-fixture = {
    description = "The evaluation's LAN: router, DNS and update server";
    wantedBy = [ "multi-user.target" ];
    before = [ "network-online.target" ];
    path = [
      pkgs.iproute2
      pkgs.dnsmasq
      python
    ];
    serviceConfig.Type = "simple";
    script = ''
      ip netns del lan 2>/dev/null || true
      ip link del lan0 2>/dev/null || true
      ip netns add lan
      ip link add lan0 type veth peer name eth0 netns lan
      ip addr add 10.88.0.1/24 dev lan0
      ip link set lan0 up
      ip -n lan addr add 10.88.0.2/24 dev eth0
      ip -n lan link set eth0 up
      ip -n lan link set lo up
      ip netns exec lan dnsmasq --keep-in-foreground --no-resolv --no-hosts \
        --listen-address=10.88.0.2 --bind-interfaces \
        --address=/updates.sekka.test/10.88.0.2 --address=/mirror.sekka.test/10.88.0.2 \
        --log-facility=- &
      exec ip netns exec lan python3 -m http.server 80 --bind 10.88.0.2 --directory ${lanWww}
    '';
  };

  # Alice's notes, served on port 8080 for her other devices.
  systemd.services.notes-web = {
    description = "Alice's notes on the local network";
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      DynamicUser = true;
      ExecStart = "${python}/bin/python3 -m http.server 8080 --directory ${notesWww}";
      Restart = "on-failure";
      RestartSec = 2;
      StartLimitBurst = 3;
    };
  };
  networking.firewall.allowedTCPPorts = [ 8080 ];

  # A backup of alice's Documents to /srv/data, which runs on demand (and
  # a timer, as a real one would; the scenarios start it by hand).
  # Test VMs take their file systems from virtualisation.fileSystems.
  virtualisation.fileSystems."/srv/data" = {
    device = "/dev/vdb";
    fsType = "ext4";
    autoFormat = true;
  };
  environment.etc."sekka-backup.env" = {
    mode = "0644";
    text = ''
      # Where sekka-backup.service writes alice's Documents.
      BACKUP_DEST=/srv/data/backups
    '';
  };
  systemd.services.sekka-backup = {
    description = "Back up alice's Documents";
    after = [ "srv-data.mount" ];
    requires = [ "srv-data.mount" ];
    path = [
      pkgs.gnutar
      pkgs.gzip
      pkgs.coreutils
    ];
    serviceConfig = {
      Type = "oneshot";
      EnvironmentFile = "/etc/sekka-backup.env";
      ExecStart = backup;
    };
  };
  systemd.timers.sekka-backup = {
    wantedBy = [ "timers.target" ];
    timerConfig.OnCalendar = "daily";
  };
  systemd.tmpfiles.rules = [
    "d /srv/data/backups 0755 root root -"
    "d /srv/data/logs 0755 root root -"
  ];

  # SSH in with a key, as alice does from her laptop.
  services.openssh.enable = true;
  services.openssh.settings.PasswordAuthentication = false;

  # Some of alice's things: documents to back up, a note, an old backup.
  systemd.services.eval-home = {
    description = "Alice's files";
    wantedBy = [ "multi-user.target" ];
    after = [
      "srv-data.mount"
      "systemd-tmpfiles-setup.service"
    ];
    requires = [ "srv-data.mount" ];
    serviceConfig.Type = "oneshot";
    serviceConfig.RemainAfterExit = true;
    script = ''
      h=/home/alice
      install -d -o alice -g users -m 0700 $h/.ssh
      install -o alice -g users -m 0600 ${sshKeys}/id_ed25519.pub $h/.ssh/authorized_keys
      # The laptop's key, which root here uses to stand in for the laptop.
      install -d -m 0700 /root/.ssh
      install -m 0600 ${sshKeys}/id_ed25519 /root/.ssh/laptop_ed25519
      install -d -o alice -g users $h/Documents $h/Downloads $h/Notes $h/Pictures
      printf 'Quarterly numbers: 12, 19, 7\n' > $h/Documents/report.txt
      # Scans: a few megabytes that don't compress, as real ones don't.
      install -d -o alice -g users $h/Documents/scans
      head -c 3000000 /dev/urandom > $h/Documents/scans/receipts-2026.pdf
      chown alice:users $h/Documents/scans/receipts-2026.pdf
      # The time zone alice chose (Aizome lets Settings own it; timedated
      # would write the same link).
      ln -sfn /etc/zoneinfo/America/Chicago /etc/localtime
      printf '# Groceries\n\n- coffee\n- bread\n' > $h/Notes/Groceries.md
      printf 'not much here\n' > $h/Downloads/readme.txt
      chown alice:users $h/Documents/report.txt $h/Notes/Groceries.md $h/Downloads/readme.txt
      mkdir -p /srv/data/backups /srv/data/logs
      if ! ls /srv/data/backups/alice-documents-20261101-*.tar.gz >/dev/null 2>&1; then
        ${pkgs.gnutar}/bin/tar -czf /srv/data/backups/alice-documents-20261101-020000.tar.gz -C $h Documents
      fi
    '';
    path = [
      pkgs.coreutils
      pkgs.gzip
    ];
  };
}
