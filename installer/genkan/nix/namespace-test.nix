# The backend's namespace tests in a disposable VM. Each re-runs itself in
# an unprivileged user and mount namespace and gives the backend the disk
# /dev/loop0 with a fake sysfs entry; the backend stats the node, which must
# be block device 7:0. A build sandbox has no block device node and can't
# make one, so the tests run here, as an ordinary user who can't open the
# node. This is a component test: no disk tool exists on the tests' path.
{ pkgs }:
let
  tests = import ./namespace-tests.nix { inherit pkgs; };
in
pkgs.testers.runNixOSTest {
  name = "genkan-namespace-tests";
  nodes.machine = {
    boot.kernelModules = [ "loop" ];
    users.users.tester.isNormalUser = true;
    environment.systemPackages = [ tests ];
  };
  # --include-ignored, so the tests still run here should they stop being
  # ignored. The deferred route's test takes that route only where memory
  # is smaller than its fake downloads' 300 GiB: true of this VM, not of
  # every build machine.
  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    assert machine.succeed("stat -c %F,%t:%T /dev/loop0").strip() == "block special file,7:0"
    machine.fail("runuser -u tester -- head -c 1 /dev/loop0")
    for binary in ["genkan-revalidation-tests", "genkan-deferred-route-tests"]:
        print(machine.succeed(
            f"runuser -u tester -- env GENKAN_FAKE_TOOLS=${tests.fakeTools} {binary} --include-ignored --nocapture 2>&1"
        ))
  '';
}
