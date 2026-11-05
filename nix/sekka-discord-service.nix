{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.sekka-discord;
  identities = {
    yuki = 980;
    yuki-tunnel = 981;
    yuki-monitor = 982;
  };
  resolver = pkgs.writeText "yuki-resolv.conf" "nameserver 1.1.1.3\noptions timeout:2 attempts:2\n";
  sandbox = {
    NoNewPrivileges = true;
    CapabilityBoundingSet = "";
    AmbientCapabilities = "";
    ProtectSystem = "strict";
    ProtectHome = true;
    PrivateTmp = true;
    PrivateDevices = true;
    ProtectKernelTunables = true;
    ProtectKernelModules = true;
    ProtectKernelLogs = true;
    ProtectControlGroups = true;
    ProtectClock = true;
    ProtectHostname = true;
    ProtectProc = "invisible";
    ProcSubset = "pid";
    RestrictSUIDSGID = true;
    RestrictRealtime = true;
    RestrictNamespaces = true;
    LockPersonality = true;
    MemoryDenyWriteExecute = true;
    SystemCallArchitectures = "native";
    SystemCallFilter = [
      "@system-service"
      "~@privileged @resources @mount @debug @obsolete"
    ];
    RestrictAddressFamilies = [
      "AF_INET"
      "AF_INET6"
    ];
    InaccessiblePaths = [
      "-/run/docker.sock"
      "-/run/containerd"
      "-/run/dbus"
      "-/run/user"
      "-/run/nix"
      "-/nix/var/nix/daemon-socket"
      "-/etc/nixos-secrets"
      cfg.credentialsDirectory
      "-/var/lib/docker"
      "-/var/lib/containers"
    ];
    BindReadOnlyPaths = [ "${resolver}:/etc/resolv.conf" ];
    UMask = "0077";
    LimitCORE = 0;
    LimitNOFILE = 1024;
    TasksMax = 64;
    MemoryMax = "512M";
    CPUQuota = "100%";
    Restart = "on-failure";
    RestartSec = 5;
  };
  network = {
    requires = [ "sekka-discord-firewall.service" ];
    after = [
      "network-online.target"
      "sekka-discord-firewall.service"
    ];
    wants = [ "network-online.target" ];
    bindsTo = [ "sekka-discord-firewall.service" ];
  };
  # Only these dedicated UIDs are filtered: CI, SSH and other host traffic are untouched.
  rules = pkgs.writeText "yuki-egress.nft" ''
    table inet yuki {
      counter blocked { }
      chain output {
        type filter hook output priority -10; policy accept;
        meta skuid { 980, 981, 982 } jump isolated
      }
      chain isolated {
        ct state established,related accept
        meta skuid 981 ip daddr 127.0.0.1 tcp dport 8787 accept
        meta skuid 982 ip daddr 127.0.0.1 tcp dport { 8787, 20241 } accept
        ip daddr 1.1.1.3 udp dport 53 accept
        ip daddr 1.1.1.3 tcp dport 53 accept
        fib daddr type local jump denied
        ip daddr { 0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16, 172.16.0.0/12, 192.0.0.0/24, 192.0.2.0/24, 192.168.0.0/16, 198.18.0.0/15, 198.51.100.0/24, 203.0.113.0/24, 224.0.0.0/3 } jump denied
        # No IPv6 egress: globally addressed LAN peers are not private prefixes.
        meta nfproto ipv6 counter name blocked reject
        meta skuid { 980, 982 } tcp dport 443 accept
        meta skuid 981 ip daddr { 198.41.192.0/24, 198.41.200.0/24 } meta l4proto { tcp, udp } th dport 7844 accept
        jump denied
      }
      chain denied {
        limit rate 5/minute burst 5 packets log prefix "YUKI_EGRESS_DENIED "
        counter name blocked reject
      }
    }
  '';
in
{
  options.services.sekka-discord = {
    enable = lib.mkEnableOption "isolated Yuki webhook receiver and security DMs";
    package = lib.mkOption {
      type = lib.types.package;
      description = "Sekka Discord package, including its independent monitor.";
    };
    alertUserId = lib.mkOption {
      type = lib.types.strMatching "[0-9]+";
      description = "Explicit Discord user ID receiving private security alerts.";
    };
    lanNetworks = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "192.168.1.0/24" ];
      description = "Home-network prefixes whose successful SSH logins are not reported. Failures from them, and loopback logins such as a tunnel connector's, still are.";
    };
    credentialsDirectory = lib.mkOption {
      type = lib.types.str;
      default = "/etc/sekka-discord";
      description = "Root-only directory containing bot-token, webhook-secret, tunnel-token and service.env; never a Nix store path.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !(lib.hasPrefix "/nix/store/" cfg.credentialsDirectory);
        message = "Yuki credentials must not be stored in the Nix store.";
      }
    ];
    users.groups = lib.mapAttrs (_: id: { gid = id; }) identities;
    users.users = lib.mapAttrs (name: id: {
      uid = id;
      isSystemUser = true;
      group = name;
      extraGroups = lib.optional (name == "yuki-monitor") "systemd-journal";
    }) identities;
    systemd.services.sekka-discord-firewall = {
      description = "Yuki fail-closed per-identity egress policy";
      before = [
        "sekka-discord.service"
        "sekka-discord-tunnel.service"
        "sekka-discord-monitor.service"
      ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      # A single nft transaction replaces our table, never the host ruleset.
      script = ''
        ${pkgs.nftables}/bin/nft list table inet yuki >/dev/null 2>&1 || ${pkgs.nftables}/bin/nft add table inet yuki
        { echo 'delete table inet yuki'; cat ${rules}; } | ${pkgs.nftables}/bin/nft -f -
      '';
    };
    systemd.services.sekka-discord = network // {
      description = "Isolated Yuki GitHub notifications";
      wantedBy = [ "multi-user.target" ];
      environment = {
        DISCORD_TOKEN_FILE = "%d/bot-token";
        GITHUB_WEBHOOK_SECRET_FILE = "%d/webhook-secret";
        SEKKA_STATE_DIR = "/var/lib/sekka-discord";
        SEKKA_LISTEN_ADDR = "127.0.0.1:8787";
      };
      serviceConfig = sandbox // {
        User = "yuki";
        Group = "yuki";
        ExecStart = "${cfg.package}/bin/sekka-discord run";
        EnvironmentFile = "${cfg.credentialsDirectory}/service.env";
        LoadCredential = [
          "bot-token:${cfg.credentialsDirectory}/bot-token"
          "webhook-secret:${cfg.credentialsDirectory}/webhook-secret"
        ];
        StateDirectory = "sekka-discord";
        StateDirectoryMode = "0700";
      };
    };
    systemd.services.sekka-discord-tunnel = network // {
      description = "Isolated Yuki Cloudflare connector";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = sandbox // {
        User = "yuki-tunnel";
        Group = "yuki-tunnel";
        LoadCredential = [ "tunnel-token:${cfg.credentialsDirectory}/tunnel-token" ];
        ExecStart = "${pkgs.cloudflared}/bin/cloudflared tunnel --no-autoupdate --edge-ip-version 4 --metrics 127.0.0.1:20241 run --token-file %d/tunnel-token";
        Restart = "always";
      };
    };
    systemd.services.sekka-discord-monitor = network // {
      description = "Yuki independent security and availability DMs";
      path = [ pkgs.systemd ];
      environment = {
        DISCORD_TOKEN_FILE = "%d/bot-token";
        DISCORD_ALERT_USER_ID = cfg.alertUserId;
        SEKKA_MONITOR_STATE_DIR = "/var/lib/sekka-discord-monitor";
        SEKKA_MONITOR_LAN_NETWORKS = lib.concatStringsSep " " cfg.lanNetworks;
        SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      };
      serviceConfig = sandbox // {
        Type = "oneshot";
        User = "yuki-monitor";
        Group = "yuki-monitor";
        SupplementaryGroups = [ "systemd-journal" ];
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_INET"
          "AF_INET6"
        ];
        # Read-only D-Bus access is not sufficient: systemctl show uses the system
        # bus, whose policy authorizes read-only operations for this identity.
        InaccessiblePaths = lib.filter (p: p != "-/run/dbus") sandbox.InaccessiblePaths;
        LoadCredential = [ "bot-token:${cfg.credentialsDirectory}/bot-token" ];
        ExecStart = "${cfg.package}/bin/sekka-discord-monitor --once";
        StateDirectory = "sekka-discord-monitor";
        StateDirectoryMode = "0700";
        TimeoutStartSec = 100;
        Restart = "no";
      };
    };
    systemd.timers.sekka-discord-monitor = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "2min";
        OnUnitInactiveSec = "60s";
        AccuracySec = "5s";
      };
    };
  };
}
