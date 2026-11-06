# Yuki's pull deployment in disposable VMs. Stand-ins replace the bot (which
# answers /healthz with its version and records its starts in its state), the
# monitor and the Cloudflare connector, and a second VM answers as GitHub's API
# over TLS at a public address, so the deploying identity's egress policy
# applies unchanged. The deployer itself is the real one from Yuki's package.
# A successful run's commit is built from a local flake, switched in and
# health-checked, keeping the bot's state and leaving the connector running; a
# run whose bot cannot stay up is rolled back and not retried; a run whose job
# was skipped is ignored; and the deployed package survives a reboot.
{
  pkgs,
  module,
  package,
}:
let
  inherit (pkgs) lib;
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

        if sys.argv[1:] == ["--help"]:
            sys.exit(0)
        with open("/var/lib/sekka-discord/starts", "a") as starts:
            starts.write("${version}\n")
        ${lib.optionalString broken "sys.exit(1)"}


        class Health(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                body = b"${version}"
                self.send_response(200 if self.path == "/healthz" else 404)
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

        if sys.argv[1:] != ["--help"]:
            with open("/var/lib/sekka-discord-monitor/version", "w") as version:
                version.write("${version}\n")
      '';
    };
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
  # The two endpoints the deployer reads, from files the test script writes.
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
in
pkgs.testers.runNixOSTest {
  name = "sekka-discord-deploy";
  nodes.github = {
    networking.firewall.allowedTCPPorts = [ 443 ];
    systemd.tmpfiles.rules = [ "d /srv/api 0755 root root -" ];
    systemd.services.fake-github-api = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.python3}/bin/python3 ${api}";
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
      lib.mkForce "${pkgs.coreutils}/bin/sleep infinity";
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
    deployer = "setpriv --reuid=yuki-deploy --regid=yuki-deploy --clear-groups -- "


    def publish(run, rev, job="success"):
        repository = {"full_name": "bitemyapp/sekka"}
        runs = {"total_count": 1, "workflow_runs": [{
            "id": run, "run_attempt": 1, "head_sha": rev, "head_branch": "main", "event": "workflow_dispatch",
            "status": "completed", "conclusion": "success", "path": ".github/workflows/yuki.yml",
            "repository": repository, "head_repository": repository}]}
        jobs = {"total_count": 1, "jobs": [{"name": "build", "status": "completed", "conclusion": job, "head_sha": rev}]}
        github.succeed(f"echo {shlex.quote(json.dumps(jobs))} > /srv/api/jobs-{run}.json")
        github.succeed(f"echo {shlex.quote(json.dumps(runs))} > /srv/api/runs.json")


    def serving(version):
        workstation.wait_until_succeeds(f"curl -sf http://127.0.0.1:8787/healthz | grep -x {version}", timeout=60)


    def show(unit, prop):
        return workstation.succeed(f"systemctl show -p {prop} --value {unit}").strip()


    def deployed():
        return workstation.succeed("readlink -f /var/lib/sekka-discord-deploy/profile").strip()


    start_all()
    github.wait_for_unit("fake-github-api.service")
    github.succeed("echo '{\"total_count\": 0, \"workflow_runs\": []}' > /srv/api/runs.json")
    github.succeed("ip addr add ${apiAddress}/32 dev eth1")
    workstation.wait_until_succeeds("ip route replace ${apiAddress}/32 dev eth1")
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

    with subtest("Without a successful run nothing is deployed"):
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        workstation.fail("test -e /var/lib/sekka-discord-deploy/profile")
        serving("seed")

    with subtest("A successful run's commit is built, switched in and verified"):
        publish(101, V1)
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        serving("v1")
        assert deployed().endswith("-sekka-discord-v1"), deployed()
        workstation.succeed(f"journalctl -u sekka-discord-deploy -o cat | grep -q 'DEPLOY succeeded rev={V1}'")
        assert workstation.succeed("cat /var/lib/sekka-discord/starts").split() == ["seed", "v1"]
        workstation.wait_until_succeeds("grep -x v1 /var/lib/sekka-discord-monitor/version")
        # The profile's generation is a garbage-collector root; the build's link is gone.
        workstation.succeed("nix-store --query --roots $(readlink -f /var/lib/sekka-discord-deploy/profile) | grep -q '/var/lib/sekka-discord-deploy/profile-1-link'")
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

    with subtest("A later run deploys, keeping the bot's state and the connector"):
        publish(104, V4)
        workstation.succeed("systemctl start sekka-discord-deploy.service")
        serving("v4")
        starts = workstation.succeed("cat /var/lib/sekka-discord/starts").split()
        assert starts[:2] == ["seed", "v1"] and "v2" in starts and starts[-1] == "v4", starts
        assert show("sekka-discord-tunnel.service", "MainPID") == tunnel

    with subtest("The deployed package survives a reboot"):
        workstation.shutdown()
        workstation.start()
        workstation.wait_for_unit("sekka-discord.service")
        serving("v4")
        workstation.wait_until_succeeds("grep -x v4 /var/lib/sekka-discord-monitor/version")
  '';
}
