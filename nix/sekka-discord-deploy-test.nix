# Yuki's pull deployment in disposable VMs. Stand-ins replace the bot (which
# answers /healthz with its version and records its starts in its state), the
# monitor and the Cloudflare connector, and a second VM answers as GitHub's API
# over TLS at a public address, so the deploying identity's egress policy
# applies unchanged. The deployer itself is the real one from Yuki's package.
# A run from before deployment was enabled is never deployed. A later
# successful run's commit is built from a local flake, switched in and
# health-checked, keeping the bot's state and leaving the connector running; a
# run whose bot cannot stay up is rolled back and not retried, to the package
# deployed before or, when there was none, to the host's own with no profile
# left to outrank it; a run whose job was skipped, that is older than one
# already deployed, or whose commit is not on main is ignored; and the deployed
# package survives a reboot.
#
# The stand-ins also say what they can reach from inside their units, which is
# where the units' own address filter applies. The second VM is the resolver
# and Cloudflare's edge at their public addresses as well, and offers one
# service at its LAN address only. Each unit reaches what it needs and not the
# LAN, with the nftables table and after the table is deleted.
{
  pkgs,
  module,
  package,
}:
let
  inherit (pkgs) lib;
  # What a stand-in can reach from inside its unit.
  reaching = ''
    import json
    import os
    import socket
    import ssl
    import urllib.request


    def reach(host, port):
        try:
            socket.create_connection((host, port), timeout=3).close()
            return "open"
        except OSError:
            return "closed"


    def fetch(url):
        try:
            context = ssl.create_default_context(cafile="/etc/ssl/certs/ca-certificates.crt")
            with urllib.request.urlopen(url, timeout=5, context=context) as answer:
                return "open" if answer.status == 200 else "closed"
        except OSError:
            return "closed"


    def resolve(name):
        try:
            return socket.getaddrinfo(name, 443, socket.AF_INET)[0][4][0]
        except OSError:
            return "closed"
  '';
  bot =
    interpreter: version: broken:
    pkgs.writeTextFile {
      name = "sekka-discord-${version}";
      executable = true;
      destination = "/bin/sekka-discord";
      text = ''
        #!${interpreter}
        import http.server
        import sys
        ${reaching}

        if sys.argv[1:] == ["--help"]:
            sys.exit(0)
        with open("/var/lib/sekka-discord/starts", "a") as starts:
            starts.write("${version}\n")
        ${lib.optionalString broken "sys.exit(1)"}


        class Health(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                body = b"${version}"
                if self.path == "/probe":
                    body = json.dumps({
                        "lan": reach("${lanAddress}", 8080),
                        "api": fetch("https://api.github.com/repos/bitemyapp/sekka/git/ref/heads/main"),
                        "resolver": resolve("${resolvedName}"),
                    }).encode()
                self.send_response(200 if self.path in ("/healthz", "/probe") else 404)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)


        http.server.HTTPServer(("127.0.0.1", 8787), Health).serve_forever()
      '';
    };
  monitor =
    interpreter: version:
    pkgs.writeTextFile {
      name = "sekka-discord-monitor-${version}";
      executable = true;
      destination = "/bin/sekka-discord-monitor";
      text = ''
        #!${interpreter}
        import sys
        ${reaching}

        if sys.argv[1:] != ["--help"]:
            with open("/var/lib/sekka-discord-monitor/version", "w") as version:
                version.write("${version}\n")
            with open("/var/lib/sekka-discord-monitor/reach.new", "w") as reached:
                json.dump({
                    "lan": reach("${lanAddress}", 8080),
                    "api": reach("api.github.com", 443),
                    "bot": reach("127.0.0.1", 8787),
                    "connector": reach("127.0.0.1", 20241),
                }, reached)
            os.replace("/var/lib/sekka-discord-monitor/reach.new", "/var/lib/sekka-discord-monitor/reach")
      '';
    };
  # The connector's stand-in answers on the metrics port the monitor checks.
  connector = pkgs.writeText "yuki-connector-stand-in.py" ''
    import http.server
    ${reaching}


    class Ready(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = b"ready"
            if self.path == "/probe":
                body = json.dumps({
                    "lan": reach("${lanAddress}", 8080),
                    "edge": reach("${edgeAddress}", 7844),
                    "origin": reach("127.0.0.1", 8787),
                    "resolver": resolve("${resolvedName}"),
                    "api": reach("${apiAddress}", 443),
                }).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)


    http.server.HTTPServer(("127.0.0.1", 20241), Ready).serve_forever()
  '';
  # Answers every address question with the API stand-in's address.
  resolver = pkgs.writeText "yuki-resolver-stand-in.py" ''
    import socket

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("${resolverAddress}", 53))
    while True:
        query, peer = sock.recvfrom(512)
        end = 12
        while query[end]:
            end += query[end] + 1
        question = query[12:end + 5]
        address = question[-4:-2] == b"\x00\x01"
        answer = b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04" + socket.inet_aton("${apiAddress}")
        sock.sendto(query[:2] + b"\x81\x80\x00\x01" + (b"\x00\x01" if address else b"\x00\x00") + b"\x00\x00\x00\x00"
                    + question + (answer if address else b""), peer)
  '';
  # This host's package: the seed stand-ins, with the real deployer.
  seed = pkgs.runCommand "sekka-discord-seed" { } ''
    mkdir -p $out/bin
    ln -s ${bot "${pkgs.python3}/bin/python3" "seed" false}/bin/sekka-discord $out/bin/
    ln -s ${monitor "${pkgs.python3}/bin/python3" "seed"}/bin/sekka-discord-monitor $out/bin/
    ln -s ${package}/bin/sekka-discord-deploy $out/bin/
  '';
  # A commit's source tree. Its flake needs nothing outside the VM: the build
  # copies the programs with a static busybox that the VM's Nix sandbox exposes.
  revision =
    version: broken:
    let
      interpreter = "/run/current-system/sw/bin/python3";
    in
    pkgs.runCommand "yuki-revision-${version}" { } ''
      mkdir -p $out/bin
      cp ${bot interpreter version broken}/bin/sekka-discord ${monitor interpreter version}/bin/sekka-discord-monitor $out/bin/
      cat > $out/flake.nix <<'EOF'
      {
        outputs = { self }: {
          packages.x86_64-linux.sekka-discord = derivation {
            name = "sekka-discord-${version}";
            system = "x86_64-linux";
            builder = "/bin/busybox";
            args = [ "sh" "-c" "/bin/busybox mkdir -p $out/bin && /bin/busybox cp ''${self}/bin/* $out/bin/" ];
          };
        };
      }
      EOF
    '';
  revisions = {
    "1111111111111111111111111111111111111111" = revision "v1" false;
    "2222222222222222222222222222222222222222" = revision "v2" true;
    "3333333333333333333333333333333333333333" = revision "v3" false;
    "4444444444444444444444444444444444444444" = revision "v4" false;
  };
  tls = pkgs.runCommand "yuki-deploy-test-tls" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir $out
    cd $out
    # Python verifies strictly: the CA needs critical constraints and key usage.
    openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.crt -days 3650 -subj "/CN=Yuki test CA" \
      -addext "basicConstraints = critical, CA:TRUE" -addext "keyUsage = critical, keyCertSign, cRLSign"
    openssl req -newkey rsa:2048 -nodes -keyout server.key -out server.csr -subj "/CN=api.github.com"
    cat > extensions <<'EOF'
    subjectAltName = DNS:api.github.com
    basicConstraints = critical, CA:FALSE
    keyUsage = critical, digitalSignature, keyEncipherment
    extendedKeyUsage = serverAuth
    subjectKeyIdentifier = hash
    authorityKeyIdentifier = keyid, issuer
    EOF
    openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 3650 -extfile extensions
  '';
  # The endpoints the deployer reads, from files the test script writes.
  api = pkgs.writeText "fake-github-api.py" ''
    import hashlib
    import http.server
    import re
    import ssl

    ROOT = "/srv/api/"


    class GitHub(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            status, body, etag = 404, b"{}", None
            if self.path.startswith("/repos/bitemyapp/sekka/actions/workflows/yuki.yml/runs?"):
                with open(ROOT + "runs.json", "rb") as source:
                    body = source.read()
                etag = '"' + hashlib.sha256(body).hexdigest() + '"'
                status = 304 if self.headers.get("If-None-Match") == etag else 200
            elif match := re.fullmatch(r"/repos/bitemyapp/sekka/actions/runs/([0-9]+)/attempts/1/jobs\?per_page=100", self.path):
                with open(ROOT + "jobs-" + match[1] + ".json", "rb") as source:
                    status, body = 200, source.read()
            elif self.path == "/repos/bitemyapp/sekka/git/ref/heads/main":
                with open(ROOT + "ref.json", "rb") as source:
                    status, body = 200, source.read()
            elif re.fullmatch(r"/repos/bitemyapp/sekka/compare/[0-9a-f]{40}\.\.\.[0-9a-f]{40}", self.path):
                with open(ROOT + "compare.json", "rb") as source:
                    status, body = 200, source.read()
            with open(ROOT + "log", "a") as log:
                log.write(f"{self.path} {status}\n")
            self.send_response(status)
            if etag:
                self.send_header("ETag", etag)
            if status != 304:
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            if status != 304:
                self.wfile.write(body)


    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain("${tls}/server.crt", "${tls}/server.key")
    server = http.server.HTTPServer(("0.0.0.0", 443), GitHub)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    server.serve_forever()
  '';
  # A public address: the deployer's egress policy refuses private ones.
  apiAddress = "140.82.112.5";
  # The resolver Yuki's units are given, and an address in the range of
  # Cloudflare's edge that the connector alone may reach.
  resolverAddress = "1.1.1.3";
  edgeAddress = "198.41.192.7";
  resolvedName = "resolved.yuki.test";
  # The second VM's own address on the network the two share: the LAN.
  lanAddress = "192.168.1.1";
  serveNothing = port: {
    wantedBy = [ "multi-user.target" ];
    serviceConfig.ExecStart = "${pkgs.python3}/bin/python3 -m http.server ${toString port} --directory ${pkgs.emptyDirectory}";
  };
in
pkgs.testers.runNixOSTest {
  name = "sekka-discord-deploy";
  nodes.github = {
    networking.firewall.allowedTCPPorts = [
      443
      7844
      8080
    ];
    networking.firewall.allowedUDPPorts = [ 53 ];
    systemd.tmpfiles.rules = [ "d /srv/api 0755 root root -" ];
    systemd.services.fake-github-api = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.python3}/bin/python3 ${api}";
    };
    # Reached at the LAN address, this is what no unit of Yuki's may reach.
    systemd.services.lan-only = serveNothing 8080;
    systemd.services.cloudflare-edge = serveNothing 7844;
    systemd.services.resolver = {
      wantedBy = [ "multi-user.target" ];
      # It cannot bind until the test script gives this VM the resolver's address.
      unitConfig.StartLimitIntervalSec = 0;
      serviceConfig = {
        ExecStart = "${pkgs.python3}/bin/python3 ${resolver}";
        Restart = "always";
        RestartSec = 1;
      };
    };
  };
  nodes.workstation = {
    imports = [ module ];
    services.sekka-discord = {
      enable = true;
      package = seed;
      alertUserId = "1";
      deploy = {
        enable = true;
        installable = "path:/srv/yuki-revisions/{rev}#sekka-discord";
      };
    };
    systemd.services.sekka-discord-tunnel.serviceConfig.ExecStart =
      lib.mkForce "${pkgs.python3}/bin/python3 ${connector}";
    systemd.timers.sekka-discord-monitor.timerConfig = {
      OnBootSec = lib.mkForce "5s";
      OnUnitInactiveSec = lib.mkForce "5s";
    };
    systemd.tmpfiles.rules = [
      "d /etc/sekka-discord 0700 root root -"
      "f /etc/sekka-discord/bot-token 0600 root root - stand-in"
      "f /etc/sekka-discord/webhook-secret 0600 root root - stand-in"
      "f /etc/sekka-discord/tunnel-token 0600 root root - stand-in"
      "f /etc/sekka-discord/service.env 0600 root root - DISCORD_CHANNEL_ID=1"
    ];
    environment.etc = lib.mapAttrs' (
      rev: tree: lib.nameValuePair "yuki-revisions/${rev}" { source = tree; }
    ) revisions;
    environment.systemPackages = [
      pkgs.python3
      pkgs.curl
      pkgs.nftables
    ];
    networking.hosts.${apiAddress} = [ "api.github.com" ];
    security.pki.certificateFiles = [ "${tls}/ca.crt" ];
    nix.settings = {
      substituters = lib.mkForce [ ];
      extra-sandbox-paths = [ "/bin/busybox=${pkgs.pkgsStatic.busybox}/bin/busybox" ];
    };
    virtualisation.memorySize = 2048;
    # Built paths must outlive the reboot, as they do on a real disk.
    virtualisation.writableStoreUseTmpfs = false;
  };
  testScript = ''
    import json
    import shlex

    V1, V2, V3, V4 = (str(n) * 40 for n in range(1, 5))
    # Where main points once it has moved on from a run's commit.
    MOVED = "9" * 40
    deployer = "setpriv --reuid=yuki-deploy --regid=yuki-deploy --clear-groups -- "


    def publish(run, rev, job="success", head=None, on_main=True):
        """A successful run of rev. main's head is rev as well, unless it has moved on to head."""
        repository = {"full_name": "bitemyapp/sekka"}
        runs = {"total_count": 1, "workflow_runs": [{
            "id": run, "run_attempt": 1, "head_sha": rev, "head_branch": "main", "event": "workflow_dispatch",
            "status": "completed", "conclusion": "success", "path": ".github/workflows/yuki.yml",
            "repository": repository, "head_repository": repository}]}
        jobs = {"total_count": 1, "jobs": [{"name": "build", "status": "completed", "conclusion": job, "head_sha": rev}]}
        ref = {"ref": "refs/heads/main", "object": {"type": "commit", "sha": head or rev}}
        # How rev compares with main's head when it is not that head: behind it, or off main.
        comparison = {"status": "behind" if on_main else "diverged", "ahead_by": 0 if on_main else 1, "behind_by": 1,
                      "merge_base_commit": {"sha": rev if on_main else V1}}
        for name, value in ((f"jobs-{run}", jobs), ("ref", ref), ("compare", comparison), ("runs", runs)):
            github.succeed(f"echo {shlex.quote(json.dumps(value))} > /srv/api/{name}.json")


    def serving(version):
        workstation.wait_until_succeeds(f"curl -sf http://127.0.0.1:8787/healthz | grep -x {version}", timeout=60)


    def show(unit, prop):
        return workstation.succeed(f"systemctl show -p {prop} --value {unit}").strip()


    def deployed():
        return workstation.succeed("readlink -f /var/lib/sekka-discord-deploy/profile").strip()


    def reached(unit):
        """What a unit's stand-in reaches from inside the unit, by its own account."""
        if unit == "monitor":
            workstation.succeed("rm -f /var/lib/sekka-discord-monitor/reach")
            workstation.wait_until_succeeds("test -e /var/lib/sekka-discord-monitor/reach", timeout=120)
            return json.loads(workstation.succeed("cat /var/lib/sekka-discord-monitor/reach"))
        port = {"bot": 8787, "connector": 20241}[unit]
        return json.loads(workstation.succeed(f"curl -sf --max-time 60 http://127.0.0.1:{port}/probe"))


    start_all()
    github.wait_for_unit("fake-github-api.service")
    # A run from before this host enabled deployment.
    publish(90, V3)
    github.wait_until_succeeds("ip -4 -o addr show dev eth1 | grep -q ' ${lanAddress}/'")
    for address in ("${apiAddress}", "${resolverAddress}", "${edgeAddress}"):
        github.succeed(f"ip addr add {address}/32 dev eth1")
        workstation.wait_until_succeeds(f"ip route replace {address}/32 dev eth1")
    github.wait_for_unit("resolver.service")
    workstation.succeed("cp -rL /etc/yuki-revisions /srv/")
    workstation.wait_for_unit("sekka-discord.service")
    serving("seed")
    workstation.wait_until_succeeds("grep -x seed /var/lib/sekka-discord-monitor/version")
    tunnel = show("sekka-discord-tunnel.service", "MainPID")

    with subtest("The deployer runs on a timer, isolated from the LAN and other units"):
        workstation.succeed("systemctl is-active sekka-discord-deploy.timer sekka-discord-deploy-restart.path")
        # From here on the test script runs each tick itself.
        workstation.succeed("systemctl stop sekka-discord-deploy.timer")
        workstation.succeed(deployer + "curl -sf --max-time 10 https://api.github.com/repos/bitemyapp/sekka/actions/workflows/yuki.yml/runs?probe")
        workstation.fail(deployer + "curl -sk --max-time 10 https://github/")
        workstation.fail(deployer + "systemctl --no-ask-password restart sekka-discord-tunnel.service")

    with subtest("A run from before deployment was enabled is not deployed"):
        for _ in range(2):
            workstation.succeed("systemctl start sekka-discord-deploy.service")
        workstation.succeed("journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY baseline run=90'")
        github.fail("grep -q /jobs /srv/api/log")
        workstation.fail("test -e /var/lib/sekka-discord-deploy/profile")
        serving("seed")

    with subtest("A first deployment that cannot stay up leaves this host's package in charge"):
        publish(100, V2)
        workstation.fail("systemctl start sekka-discord-deploy.service")
        serving("seed")
        workstation.succeed(f"journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY FAILED rev={V2}.*restored this host.s package'")
        # No profile is left for Yuki's launchers to prefer to the host's package.
        workstation.fail("ls /var/lib/sekka-discord-deploy | grep -q '^profile'")
        workstation.succeed("test -e /var/lib/sekka-discord-deploy/state.json")

    with subtest("A successful run's commit is built, switched in and verified"):
        publish(101, V1)
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        serving("v1")
        assert deployed().endswith("-sekka-discord-v1"), deployed()
        workstation.succeed(f"journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY succeeded rev={V1}'")
        starts = workstation.succeed("cat /var/lib/sekka-discord/starts").split()
        assert starts[0] == "seed" and "v2" in starts and starts[-2:] == ["seed", "v1"], starts
        workstation.wait_until_succeeds("grep -x v1 /var/lib/sekka-discord-monitor/version")
        # The profile's generation is a garbage-collector root; the build's link is gone.
        workstation.succeed("nix-store --query --roots $(readlink -f /var/lib/sekka-discord-deploy/profile) | grep -q '/var/lib/sekka-discord-deploy/profile-[0-9]*-link'")
        workstation.fail("test -e /var/lib/sekka-discord-deploy/candidate")
        assert workstation.succeed("stat -c %a /var/lib/sekka-discord-deploy/state.json").strip() == "600"
        assert show("sekka-discord-tunnel.service", "MainPID") == tunnel

    with subtest("An unchanged runs list deploys nothing"):
        invocation = show("sekka-discord.service", "InvocationID")
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        github.succeed("tail -n 1 /srv/api/log | grep -q ' 304$'")
        assert show("sekka-discord.service", "InvocationID") == invocation

    with subtest("A run whose bot cannot stay up is rolled back and not retried"):
        publish(102, V2)
        workstation.fail("systemctl start sekka-discord-deploy.service")
        serving("v1")
        assert deployed().endswith("-sekka-discord-v1"), deployed()
        workstation.succeed(f"journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY FAILED rev={V2}'")
        invocation = show("sekka-discord.service", "InvocationID")
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        assert show("sekka-discord.service", "InvocationID") == invocation

    with subtest("A run whose job was skipped is not deployed"):
        publish(103, V3, job="skipped")
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        workstation.succeed("journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY skipped run=103'")
        assert show("sekka-discord.service", "InvocationID") == invocation
        serving("v1")

    with subtest("A later run deploys though main has moved on, keeping the bot's state and the connector"):
        publish(104, V4, head=MOVED)
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        serving("v4")
        github.succeed(f"grep -q '/compare/{MOVED}\\.\\.\\.{V4} 200' /srv/api/log")
        starts = workstation.succeed("cat /var/lib/sekka-discord/starts").split()
        assert starts[0] == "seed" and "v1" in starts and starts[-1] == "v4", starts
        assert show("sekka-discord-tunnel.service", "MainPID") == tunnel

    with subtest("A run that is the latest only because a later one is gone is refused"):
        invocation = show("sekka-discord.service", "InvocationID")
        publish(101, V1)
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        workstation.succeed("journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY FAILED run=101: older than run 104'")
        assert show("sekka-discord.service", "InvocationID") == invocation

    with subtest("A run on a tag named main, of a commit that is not on main, is refused"):
        publish(105, V3, head=V4, on_main=False)
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        workstation.succeed(f"journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY FAILED rev={V3} run=105: the commit is not on main'")
        assert show("sekka-discord.service", "InvocationID") == invocation
        assert deployed().endswith("-sekka-discord-v4"), deployed()
        serving("v4")

    bot = {"lan": "closed", "api": "open", "resolver": "${apiAddress}"}
    connector = {"lan": "closed", "edge": "open", "origin": "open", "resolver": "${apiAddress}", "api": "closed"}
    monitor = {"lan": "closed", "api": "open", "bot": "open", "connector": "open"}
    units = ("sekka-discord", "sekka-discord-tunnel", "sekka-discord-monitor", "sekka-discord-deploy")

    with subtest("Each unit reaches what it needs and none reaches the LAN"):
        workstation.succeed("curl -sf --max-time 10 http://${lanAddress}:8080/")
        assert reached("bot") == bot, reached("bot")
        assert reached("connector") == connector, reached("connector")
        assert reached("monitor") == monitor, reached("monitor")
        for unit in units:
            denied = show(unit + ".service", "IPAddressDeny").split()
            assert "192.168.0.0/16" in denied and "::/0" in denied and "127.0.0.0/8" not in denied, denied

    with subtest("Without the nftables table the LAN stays out of reach and the rest within it"):
        workstation.succeed("nft delete table inet yuki")
        # The table was all that refused this identity outside a unit.
        workstation.succeed(deployer + "curl -sf --max-time 10 http://${lanAddress}:8080/")
        assert reached("bot") == bot, reached("bot")
        # Ports and each identity's own destinations were the table's alone to
        # enforce: the connector now reaches the API's port, as it could not.
        assert reached("connector") == connector | {"api": "open"}, reached("connector")
        assert reached("monitor") == monitor, reached("monitor")
        for unit in units[:2]:
            assert show(unit + ".service", "ActiveState") == "active"

    with subtest("The deployed package survives a reboot"):
        workstation.shutdown()
        workstation.start()
        workstation.wait_for_unit("sekka-discord.service")
        serving("v4")
        workstation.wait_until_succeeds("grep -x v4 /var/lib/sekka-discord-monitor/version")
        # The boot brought the table back before any unit of Yuki's started.
        workstation.succeed("nft list table inet yuki")
  '';
}
