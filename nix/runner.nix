# The release workflow's runner: GitHub Actions jobs on a builder with
# Docker and a Nix daemon (wx-workstation, see DEVELOPERS.md), each in a
# sandbox of its own.
#
# A job runs in a fresh container, as an unprivileged user with every
# capability dropped, a read-only root and no way to gain privileges. It sees
# the host's Nix store read-only and its daemon's socket, so builds run in
# the daemon's own sandbox as an untrusted user. The only persistent job
# mounts are bounded GC roots and read-only Git objects. The runner in it
# is registered for that one job (GitHub's
# just-in-time configuration) and the container and its workspace go with
# the job.
#
# The supervisor, in a container of its own that Docker restarts, asks
# GitHub for each job's registration with a token only it can read, and
# starts the job's container. The images hold no store paths: both run
# programs from the host's store, kept by the `runner` package's GC root.
{
  pkgs,
  lib,
  nixPackage,
}:
let
  repository = "bitemyapp/sekka";
  # Jobs' user and group: nobody else's on the builder, apart from its Nix
  # build users' ids (from 30000), and reserved there by
  # scripts/runner-host-setup.sh.
  uid = 30500;

  # What a job's steps find on PATH.
  tools = pkgs.buildEnv {
    name = "sekka-runner-tools";
    paths = with pkgs; [
      bashInteractive
      coreutils
      diffutils
      findutils
      gawk
      gnugrep
      gnused
      gnutar
      gzip
      xz
      zstd
      which
      procps
      git
      gh
      python3
      curl
      jq
      # The host runs Determinate Nix. Its cancellation status is not decoded
      # by the upstream client, which hides the original failed derivation.
      nixPackage
      # Streams evaluation (nix-eval-jobs, on upstream Nix's evaluator) into
      # builds, which go through nixPackage (scripts/ci-retain.py).
      nix-fast-build
      cacert
    ];
  };

  # Before each job the runner runs this: it takes only the repository's
  # workflows from main, started by hand, on their schedule or by a push to
  # main, plus main's trusted-PR workflow after its permission gate. For
  # anything else, a pull request's own workflow above all, it ends every process of the job but
  # the runner itself, which reports the job failed and leaves: a failed
  # step alone would not stop later steps that run always(). This holds
  # whatever GitHub's settings let through.
  admit = pkgs.writeShellScript "sekka-runner-admit.sh" ''
    source ${../scripts/runner-admission.sh}
    job="''${GITHUB_REPOSITORY:-?} ''${GITHUB_EVENT_NAME:-?} ''${GITHUB_REF:-?}"
    if ! sekka_runner_admitted; then
      echo "::error::This runner requires an admitted main workflow of ${repository}, not: $job"
      kill -KILL -1
      exit 1
    fi
    # Expire CI roots even in jobs that build nothing, then give checkout a
    # private repository over the supervisor's read-only Git objects.
    ${pkgs.python3}/bin/python3 ${../scripts/ci-retain.py} prune
    ${pkgs.bash}/bin/bash ${../scripts/runner-git-cache.sh} seed
  '';

  # A job: the runner, registered for it alone, then gone with its container.
  job = pkgs.writeShellApplication {
    name = "sekka-runner-job";
    runtimeInputs = [ pkgs.github-runner ];
    text = ''
      config="''${RUNNER_JITCONFIG:?The supervisor passes the registration for this job}"
      unset RUNNER_JITCONFIG
      exec Runner.Listener run --jitconfig "$config"
    '';
  };

  # Files a job's root has: users for the unprivileged uid, a shell where
  # scripts expect one, and the CA bundle.
  rootFiles = ''
    mkdir -p etc etc/ssl/certs bin usr/bin tmp home/runner
    printf 'root:x:0:0::/root:/bin/sh\nrunner:x:${toString uid}:${toString uid}::/home/runner:/bin/sh\n' > etc/passwd
    printf 'root:x:0:\nrunner:x:${toString uid}:\n' > etc/group
    printf 'hosts: files dns\n' > etc/nsswitch.conf
    ln -s ${pkgs.bashInteractive}/bin/bash bin/sh
    ln -s ${pkgs.bashInteractive}/bin/bash bin/bash
    ln -s ${pkgs.coreutils}/bin/env usr/bin/env
    ln -s ${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt etc/ssl/certs/ca-certificates.crt
  '';
  environment = [
    "PATH=${tools}/bin"
    "HOME=/home/runner"
    "LANG=C.UTF-8"
    "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
    "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
    # Builds go to the host's daemon, as an untrusted user. The daemon takes
    # max-jobs and cores from its client, and Nix's default of one job at a
    # time made a nightly build its thousands of derivations in sequence.
    # Each of the two slots allows up to 16 builds, 16 cores per build, on
    # the 128-thread builder: the former single runner's 32 when both are
    # busy. Eight evaluator threads measured fastest (DEVELOPERS.md).
    # Workflows set no NIX_CONFIG of their own, which would replace this
    # whole value.
    "NIX_REMOTE=daemon"
    "NIX_CONFIG=experimental-features = nix-command flakes\nmax-jobs = 16\ncores = 16\neval-cores = 8"
    "ACTIONS_RUNNER_HOOK_JOB_STARTED=${admit}"
  ];

  jobImage = pkgs.dockerTools.buildLayeredImage {
    name = "sekka-runner";
    tag = "latest";
    includeStorePaths = false;
    fakeRootCommands = rootFiles + ''
      chown ${toString uid}:${toString uid} home/runner
      chmod 1777 tmp
    '';
    config = {
      Entrypoint = [ (lib.getExe job) ];
      User = "${toString uid}:${toString uid}";
      WorkingDir = "/home/runner";
      Env = environment;
    };
  };

  # Two interchangeable instances of this image. Each serializes only
  # its own jobs and Git cache; both reuse the host's Nix store and GC roots.
  supervisor = pkgs.writeShellApplication {
    name = "sekka-runner-supervisor";
    runtimeInputs = with pkgs; [
      bash
      coreutils
      curl
      jq
      git
      util-linux
      docker-client
    ];
    runtimeEnv = {
      SEKKA_RUNNER_UID = toString uid;
      SEKKA_RUNNER_GIT_CACHE_SCRIPT = "${../scripts/runner-git-cache.sh}";
    };
    text = builtins.readFile ../scripts/runner-supervisor.sh;
  };

  supervisorImage = pkgs.dockerTools.buildLayeredImage {
    name = "sekka-runner-supervisor";
    tag = "latest";
    includeStorePaths = false;
    fakeRootCommands = rootFiles + ''
      chmod 1777 tmp
    '';
    config = {
      Entrypoint = [ (lib.getExe supervisor) ];
      Env = [
        "PATH=${pkgs.coreutils}/bin"
        "HOME=/tmp"
        "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      ];
    };
  };
in
# Everything the containers run, kept alive by one GC root on the builder:
# the images refer to store paths without holding them.
(pkgs.linkFarm "sekka-runner" {
  "job-image.tar.gz" = jobImage;
  "supervisor-image.tar.gz" = supervisorImage;
  job = job;
  "admit.sh" = admit;
  supervisor = supervisor;
  tools = tools;
}).overrideAttrs
  (_: {
    passthru = { inherit tools nixPackage; };
  })
