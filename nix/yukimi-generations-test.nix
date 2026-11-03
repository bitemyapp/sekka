# Deleting single system generations through Yukimi's helper, in a disposable
# VM with real systemd-boot entries and a store on its own ext4 disk: the
# profile links, boot entries and kept configurations that go and stay, the
# refusals, and the disk space garbage collection gives back, measured
# against Yukimi's estimate (and against the closure size it showed before).
{
  pkgs,
  yukimi,
  yukimiModule,
}:
let
  # What a generation has of its own: incompressible data, small files (a
  # block each on disk, far less as NAR), a copy of a file the running system
  # has (hard-linked to it once the store is optimised, so it stays), and a
  # file generation 4's payload has too (it goes only with both).
  payload =
    n:
    pkgs.runCommand "yukimi-test-payload-${n}" { } ''
      mkdir -p $out/share/yukimi-test/small
      head -c ${toString (2 + 2 * (pkgs.lib.toInt n))}M /dev/urandom > $out/share/yukimi-test/unique.bin
      for i in $(seq 1 300); do
        echo "generation ${n}, file $i" > $out/share/yukimi-test/small/$i
      done
      cp ${pkgs.coreutils}/bin/coreutils $out/share/yukimi-test/coreutils-copy
      head -c 3M /dev/zero | tr '\0' t > $out/share/yukimi-test/twin
    '';
  blob =
    name: size:
    pkgs.runCommand "yukimi-test-${name}" { } ''
      mkdir -p $out/share/yukimi-test
      head -c ${toString size}M /dev/urandom > $out/share/yukimi-test/${name}.bin
    '';
  # Generations 3 and 4 share this.
  shared = blob "shared" 2;
  # Generation 3 has this, and so does a user's profile.
  keptByUser = blob "kept-by-user" 1;
  # Generation 3 has this, and a program runs in it.
  busy = blob "busy" 1;
  variant =
    n:
    { lib, ... }:
    {
      environment.etc."yukimi-test-generation".text = n;
      environment.systemPackages = [
        (payload n)
      ]
      ++ lib.optional (n == "3" || n == "4") shared
      ++ lib.optionals (n == "3") [
        keptByUser
        busy
      ];
    };
in
pkgs.testers.runNixOSTest {
  name = "yukimi-delete-generation";
  globalTimeout = 900;
  nodes.machine =
    {
      config,
      lib,
      extendModules,
      ...
    }:
    {
      imports = [ yukimiModule ];
      virtualisation = {
        memorySize = 2048;
        # The store on the VM's own disk, not shared with the host: deleting
        # a path gives its blocks back.
        useBootLoader = true;
        useEFIBoot = true;
        # The other generations, built beside this one; they become
        # generations in the test.
        additionalPaths = lib.attrValues config.system.build.yukimiGenerations;
      };
      system.build.yukimiGenerations = lib.genAttrs [ "2" "3" "4" "5" ] (
        n: (extendModules { modules = [ (variant n) ]; }).config.system.build.toplevel
      );
      boot.loader.systemd-boot.enable = true;
      boot.loader.efi.canTouchEfiVariables = true;
      boot.loader.timeout = 1;
      system.switch.enable = true;
      nix.enable = true;
      nix.settings.auto-optimise-store = true;
      # Nothing else writes to the disk while space is measured.
      services.journald.settings.Journal.Storage = "volatile";
      # Yukimi's activation identity names the graphics drivers, which this
      # machine without a display has none of.
      hardware.graphics.package = pkgs.emptyDirectory;
      hardware.graphics.package32 = pkgs.emptyDirectory;
      environment.etc."machine-id".text = "1234567890abcdef1234567890abcdef\n";
      programs.yukimi = {
        enable = true;
        package = yukimi;
      };
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
      };
    };
  testScript =
    { nodes, ... }:
    let
      gens = nodes.machine.system.build.yukimiGenerations;
    in
    ''
      import json

      gens = {
          2: "${gens."2"}",
          3: "${gens."3"}",
          4: "${gens."4"}",
          5: "${gens."5"}",
      }
      payload3 = "${payload "3"}"
      kept_by_user, busy, shared = "${keptByUser}", "${busy}", "${shared}"

      def used():
          """Bytes in use on the store's filesystem."""
          machine.succeed("sync")
          blocks, free, size = machine.succeed("stat -f -c '%b %f %S' /nix/store").split()
          return (int(blocks) - int(free)) * int(size)

      def links():
          return sorted(machine.succeed("ls /nix/var/nix/profiles | grep -o '^system-[0-9]*-link'").split())

      def entries():
          """The generations the boot menu lists, from its entries' titles
          (`version Generation <n> ...`)."""
          versions = machine.succeed("cat /boot/loader/entries/*.conf | grep '^version Generation '")
          return sorted(int(line.split()[2]) for line in versions.splitlines())

      def kept():
          return sorted(machine.succeed("ls /var/lib/yukimi/configurations").split())

      def estimate(numbers, user=None):
          command = f"yukimi-helper estimate {numbers}"
          if user:
              command = f"su {user} -c '{command}'"
          return json.loads(machine.succeed(command))

      def mib(n):
          return f"{n / 1048576:.2f} MiB"

      machine.start()
      machine.wait_for_unit("multi-user.target")
      base = machine.succeed("readlink -f /run/current-system").strip()

      with subtest("generations 2 to 5, as nixos-rebuild leaves them"):
          for n in sorted(gens):
              machine.succeed(f"nix-env -p /nix/var/nix/profiles/system --set {gens[n]}")
          # 5 is selected for the next start; 1 runs and was started with.
          machine.succeed("/nix/var/nix/profiles/system/bin/switch-to-configuration boot")
          assert links() == [f"system-{n}-link" for n in range(1, 6)], links()
          assert entries() == list(range(1, 6)), entries()
          for n in range(1, 6):
              machine.succeed(f"mkdir -p /var/lib/yukimi/configurations/generation-{n}")
              machine.succeed(f"echo '{{ }}' > /var/lib/yukimi/configurations/generation-{n}/configuration.nix")
          # Other roots: a user's profile has one of 3's packages, and a
          # program of the user's runs in another.
          machine.succeed("mkdir -p /nix/var/nix/profiles/per-user/demo")
          machine.succeed(f"nix-env -p /nix/var/nix/profiles/per-user/demo/profile --set {kept_by_user}")
          machine.succeed(f"systemd-run --uid=demo --unit=yukimi-busy -p WorkingDirectory={busy} sleep infinity")
          # Optimise the store, as auto-optimise-store does for new paths,
          # then collect what is garbage already, so that what follows
          # measures only what deleting gives back.
          machine.succeed("nix-store --optimise")
          machine.succeed("nix-store --gc")

      with subtest("the generations in use, and anything but a generation number, are refused"):
          for args, why in [
              ("1", "running system"),
              ("5", "selected for the next start"),
              ("9", "no generation 9"),
              ("03", "Not a generation number"),
              ("3x", "Not a generation number"),
              ("3 3", "named twice"),
              ("-- 3", "Not a generation number"),
              ("", "needs the numbers"),
          ]:
              report = json.loads(machine.fail(f"yukimi-helper delete {args}"))
              assert not report["ok"] and why in report["error"], (args, report)
          assert links() == [f"system-{n}-link" for n in range(1, 6)], links()
          assert entries() == list(range(1, 6)), entries()
          assert kept() == [f"generation-{n}" for n in range(1, 6)], kept()
          machine.fail("su demo -c 'yukimi-helper delete 3'")
          assert links() == [f"system-{n}-link" for n in range(1, 6)], links()

      with subtest("delete 3 and collect garbage: what comes back matches the estimate"):
          as_user = estimate("3", user="demo")
          as_root = estimate("3")
          both = estimate("3 4")
          print("estimate for 3 as demo:", json.dumps(as_user))
          print("estimate for 3 as root:", json.dumps(as_root))
          print("estimate for 3 and 4:", json.dumps(both))
          assert as_user["roots"] == "nix" and as_root["roots"] == "nix"
          assert as_root["optimised"], as_root
          # The daemon tells anyone what running programs use.
          assert abs(as_user["disk"] - as_root["disk"]) < 64 * 1024, (as_user, as_root)
          # Before, Yukimi showed the generation's whole closure.
          closure = int(machine.succeed("nix path-info -S /nix/var/nix/profiles/system-3-link").split()[1])
          assert closure == as_root["closure"], (closure, as_root)
          before = used()
          report = json.loads(machine.succeed("yukimi-helper delete 3 --collect-garbage"))
          after = used()
          print("delete 3:", json.dumps(report))
          assert report["ok"] and report["deleted"] == [3] and report["collected"], report
          freed = before - after
          estimate3 = as_root["disk"]
          print(
              f"generation 3: freed {mib(freed)} on disk; Yukimi showed {mib(closure)} before "
              f"({(closure - freed) / freed:+.1%}), its NAR estimate {mib(as_root['nar'])} "
              f"({(as_root['nar'] - freed) / freed:+.1%}), its disk estimate {mib(estimate3)} "
              f"({(estimate3 - freed) / freed:+.1%})"
          )
          assert abs(estimate3 - freed) <= 0.03 * freed + 256 * 1024, (estimate3, freed)
          assert abs(report["freed"] - freed) <= 0.03 * freed + 256 * 1024, (report["freed"], freed)
          # Deleting 3 and 4 together gives back what they share, too.
          assert both["disk"] > as_root["disk"], (both, as_root)

      with subtest("3 is gone from the profile, the boot menu and the kept configurations"):
          assert links() == ["system-1-link", "system-2-link", "system-4-link", "system-5-link"], links()
          assert entries() == [1, 2, 4, 5], entries()
          assert kept() == ["generation-1", "generation-2", "generation-4", "generation-5"], kept()
          assert machine.succeed("readlink /nix/var/nix/profiles/system").strip() == "system-5-link"
          assert machine.succeed("readlink -f /run/current-system").strip() == base
          machine.fail(f"test -e {gens[3]}")
          machine.fail(f"test -e {payload3}")
          # What others keep stays.
          for path in [kept_by_user, busy, shared]:
              machine.succeed(f"nix-store --check-validity {path}")
              machine.succeed(f"test -e {path}")

      with subtest("delete 2 alone: its space comes back only when garbage is collected"):
          estimate2 = estimate("2")
          before = used()
          report = json.loads(machine.succeed("yukimi-helper delete 2"))
          print("delete 2:", json.dumps(report))
          assert report["ok"] and report["deleted"] == [2] and not report["collected"], report
          assert "freed" not in report, report
          assert links() == ["system-1-link", "system-4-link", "system-5-link"], links()
          assert 2 not in entries(), entries()
          assert "generation-2" not in kept(), kept()
          machine.succeed(f"test -e {gens[2]}")
          after = used()
          assert abs(after - before) < 1024 * 1024, (before, after)
          # Now it is garbage, which Yukimi says, and clean-up collects.
          assert abs(estimate("4")["garbage"] - estimate2["disk"]) <= 0.01 * estimate2["disk"] + 64 * 1024
          report = json.loads(machine.succeed("yukimi-helper clean"))
          freed = after - used()
          print(f"generation 2: freed {mib(freed)}; estimated {mib(estimate2['disk'])}; closure {mib(estimate2['closure'])}")
          assert report["ok"], report
          assert abs(estimate2["disk"] - freed) <= 0.03 * freed + 256 * 1024, (estimate2, freed)
          machine.fail(f"test -e {gens[2]}")
          assert entries() == [1, 4, 5], entries()
    '';
}
