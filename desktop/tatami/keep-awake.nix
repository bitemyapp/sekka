# The inhibitor belongs to the session, never to Waybar or a workload app.
{
  lib,
  pkgs,
  tool,
}:
{
  description = "Tatami verified Keep Awake";
  wantedBy = [ "wayland-session@tatami.target" ];
  partOf = [ "graphical-session.target" ];
  after = [
    "wayland-wm@tatami.service"
    "wayland-session-waitenv.service"
  ];
  unitConfig.StartLimitIntervalSec = 0;
  enableDefaultPath = false;
  environment.PATH = "/run/wrappers/bin:/etc/profiles/per-user/%u/bin:/run/current-system/sw/bin";
  serviceConfig = {
    ExecCondition = "${lib.getExe tool} awake requested";
    ExecStart = lib.escapeShellArgs [
      "${pkgs.systemd}/bin/systemd-inhibit"
      "--what=sleep:idle:handle-lid-switch"
      "--who=Tatami Keep Awake"
      "--why=Keep Awake is enabled; turn it off in Tatami to allow sleep"
      "--mode=block"
      "${lib.getExe tool}"
      "awake"
      "hold"
    ];
    Type = "exec";
    # systemd-inhibit sanitizes NOTIFY_SOCKET in its child. Verify the
    # inhibitor owned by MAINPID before systemd completes the start job.
    ExecStartPost = "${lib.getExe tool} awake verify";
    TimeoutStartSec = 15;
    Restart = "always";
    RestartSec = 1;
  };
}
