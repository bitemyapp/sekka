# The release workflow's runner: GitHub Actions jobs on a builder with
# Docker and a Nix daemon (wx-workstation, see DEVELOPERS.md), each in a
# sandbox of its own.
#
# A job runs in a fresh container, as an unprivileged user with every
# capability dropped, a read-only root and no way to gain privileges. It sees
# the host's Nix store read-only and its daemon's socket, so builds run in
# the daemon's own sandbox as an untrusted user, and nothing else of the
# host. The runner in it is registered for that one job (GitHub's
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
  labels = [
    "self-hosted"
    "linux"
    "x64"
    "sekka-builder"
  ];
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
    # time made a nightly build its thousands of derivations in sequence:
    # up to 32 at once here, 16 cores each, on the 128-core builder.
    "NIX_REMOTE=daemon"
    "NIX_CONFIG=experimental-features = nix-command flakes\nmax-jobs = 32\ncores = 16"
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

  # The supervisor: one job's runner at a time, for as long as Docker keeps
  # it running.
  supervisor = pkgs.writeShellApplication {
    name = "sekka-runner-supervisor";
    runtimeInputs = with pkgs; [
      coreutils
      curl
      jq
      docker-client
    ];
    text = ''
      token=/run/secrets/token
      secrets=$(mktemp -d)
      trap 'rm -rf "$secrets"' EXIT
      while true; do
        # A job that outlived an earlier supervisor finishes first.
        docker wait sekka-runner-job >/dev/null 2>&1 || true
        if [ ! -r "$token" ]; then
          echo "No token at $token yet; checking again in a minute."
          sleep 60
          continue
        fi
        # The token stays in files: never on a command line, where other
        # users of the host could read it.
        printf 'Authorization: Bearer %s\n' "$(cat "$token")" > "$secrets/headers"
        name="wx-workstation-$(date -u +%Y%m%dT%H%M%S)"
        request=$(jq -cn --arg name "$name" --argjson labels '${builtins.toJSON labels}' \
          '{name: $name, runner_group_id: 1, labels: $labels, work_folder: "_work"}')
        if ! curl -fsS -X POST \
          -H @"$secrets/headers" \
          -H "Accept: application/vnd.github+json" \
          -H "X-GitHub-Api-Version: 2022-11-28" \
          --data "$request" \
          https://api.github.com/repos/${repository}/actions/runners/generate-jitconfig \
          | jq -er '"RUNNER_JITCONFIG=" + .encoded_jit_config' > "$secrets/job.env"; then
          echo "GitHub did not register a runner; trying again in a minute."
          sleep 60
          continue
        fi
        echo "$name is waiting for a job."
        docker run --rm --name sekka-runner-job \
          --env-file "$secrets/job.env" \
          --user ${toString uid}:${toString uid} \
          --cap-drop ALL \
          --security-opt no-new-privileges \
          --read-only \
          --tmpfs /tmp:rw,exec,nosuid,nodev,size=8g \
          --mount type=volume,dst=/home/runner \
          --volume /nix/store:/nix/store:ro \
          --volume /nix/var/nix/daemon-socket:/nix/var/nix/daemon-socket:ro \
          --pids-limit 16384 \
          --memory 64g \
          --cpus 32 \
          sekka-runner:latest \
          || echo "$name ended with status $?."
        rm -f "$secrets/job.env"
        sleep 5
      done
    '';
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
