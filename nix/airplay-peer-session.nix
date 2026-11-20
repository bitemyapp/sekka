{
  lib,
  writeShellApplication,
  writeShellScript,
  coreutils,
  systemd,
  iptables,
  airplay-radio,
  intelDriver ? null,
  intelPowerScheme ? null,
}:
let
  driverState = "/run/sekka-airplay-intel-trial";
  waitDriver = ''
    udevadm settle --timeout=15
    for attempt in $(seq 1 100); do
      state=$(nmcli -g GENERAL.STATE device show wlp0s20f3 2>/dev/null || true)
      if [[ "$state" == 30* || "$state" == 100* ]]; then break; fi
      sleep 0.1
    done
  '';
  loadDriver = lib.optionalString (intelDriver != null) ''
    if [[ "$(uname -r)" != ${lib.escapeShellArg intelDriver.modDirVersion} ]]; then
      echo 'The test driver does not match the running kernel.' >&2
      exit 1
    fi
    if [[ -e ${driverState} ]]; then
      result=0
      ${restoreDriver}
      if (( result != 0 )); then exit "$result"; fi
    fi
    if [[ ! -d /sys/module/iwlmld ]]; then
      echo 'The Intel driver is unavailable.' >&2
      exit 1
    fi
    printf '%s\n' '{"state":"starting","message":"Loading the Intel driver correction for this trial"}'
    mkdir -m700 ${driverState}
    cat /sys/module/iwlmld/parameters/power_scheme > ${driverState}/power-scheme
    nmcli -g GENERAL.CON-UUID device show wlp0s20f3 > ${driverState}/connection
    # Mark before unloading: ExecStopPost must also recover a failed insmod.
    touch ${driverState}/restore
    # Remove only the op-mode: modprobe -r also unloads mac80211, iwlwifi
    # and PTP, leaving insmod without its required exported symbols.
    rmmod iwlmld
    insmod ${intelDriver}/lib/modules/${intelDriver.modDirVersion}/updates/iwlmld.ko \
      power_scheme=${
        if intelPowerScheme == null then
          ''"$(cat ${driverState}/power-scheme)"''
        else
          toString intelPowerScheme
      } \
      ${lib.escapeShellArgs (intelDriver.loadParameters or [ ])}
    printf '{"state":"driver_loaded","version":"%s"}\n' "$(cat /sys/module/iwlmld/version)"
    ${waitDriver}
    nmcli --wait 20 connection up uuid "$(cat ${driverState}/connection)" >/dev/null
  '';
  restoreDriver = lib.optionalString (intelDriver != null) ''
    if [[ -f ${driverState}/restore ]]; then
      systemctl stop wpa_supplicant-wlp0s20f3.service 2>/dev/null || true
      if [[ -d /sys/module/iwlmld ]]; then rmmod iwlmld || result=$?; fi
      modprobe -d /run/booted-system/kernel-modules iwlmld \
        power_scheme="$(cat ${driverState}/power-scheme)" || result=$?
      ${waitDriver}
      nmcli --wait 25 connection up uuid "$(cat ${driverState}/connection)" >/dev/null || result=$?
      if (( result == 0 )); then rm -rf ${driverState}; fi
    fi
  '';
  prepare = writeShellScript "sekka-airplay-peer-prepare" ''
    set -eu
    export PATH=/run/current-system/sw/bin:${
      lib.makeBinPath [
        coreutils
        systemd
        iptables
      ]
    }
    ${loadDriver}
    mkdir -p /run/udev/rules.d /run/NetworkManager/conf.d /run/systemd/network
    cat > /run/systemd/network/10-sekka-airplay-awdl.link <<'LINK'
    [Match]
    OriginalName=awdl0
    [Link]
    MACAddressPolicy=none
    LINK
    # NixOS otherwise restarts wpa_supplicant for every temporary interface.
    cat > /run/udev/rules.d/00-sekka-airplay-peer.rules <<'RULE'
    SUBSYSTEM=="net", KERNEL=="awdl0|owlymon0|owlyibss0|owlyap0", RUN:="${coreutils}/bin/true"
    RULE
    cat > /run/NetworkManager/conf.d/90-sekka-airplay-peer.conf <<'CONF'
    [device-sekka-airplay-peer]
    match-device=interface-name:awdl0;interface-name:owlymon0;interface-name:owlyibss0;interface-name:owlyap0
    managed=0
    CONF
    udevadm control --reload
    nmcli general reload conf
    allow_firewall=true
    if [[ -e /etc/sekka/airplay/open-firewall ]]; then
      allow_firewall=$(cat /etc/sekka/airplay/open-firewall)
    fi
    if [[ "$allow_firewall" == true ]] && ip6tables -nL nixos-fw >/dev/null 2>&1; then
      ip6tables -I nixos-fw 1 -i awdl0 -p udp -m multiport --dports 5353,60000:60002 -m comment --comment sekka-airplay-peer -j nixos-fw-accept
    fi
  '';
  cleanup = writeShellScript "sekka-airplay-peer-cleanup" ''
    export PATH=/run/current-system/sw/bin:${
      lib.makeBinPath [
        coreutils
        systemd
        iptables
      ]
    }
    result=0
    ${airplay-radio}/bin/sekka-airplay-radio recover || result=$?
    ip6tables -D nixos-fw -i awdl0 -p udp -m multiport --dports 5353,60000:60002 -m comment --comment sekka-airplay-peer -j nixos-fw-accept 2>/dev/null || true
    rm -f /run/udev/rules.d/00-sekka-airplay-peer.rules /run/NetworkManager/conf.d/90-sekka-airplay-peer.conf /run/systemd/network/10-sekka-airplay-awdl.link
    udevadm control --reload || result=$?
    nmcli general reload conf || result=$?
    ${restoreDriver}
    exit "$result"
  '';
in
assert intelPowerScheme == null || intelPowerScheme == 1;
writeShellApplication {
  name = "sekka-airplay-peer-session";
  runtimeInputs = [
    coreutils
    systemd
  ];
  text = ''
    if [[ "''${1:-}" == probe && $# == 1 ]]; then
      exec ${airplay-radio}/bin/sekka-airplay-radio probe
    fi
    if [[ $# -lt 2 || $# -gt 3 || ( "$1" != session && "$1" != observe && "$1" != monitor && "$1" != trial ) || ! "$2" =~ ^[0-9]+$ ]] || (( 10#$2 < 1 || 10#$2 > 14400 )); then
      echo 'Usage: sekka-airplay-peer-session probe | session SECONDS [6|44|149] | observe SECONDS' >&2
      exit 2
    fi
    if [[ "$1" == trial ]]; then
      if [[ $# != 3 || ! "$3" =~ ^[a-z0-9-]+$ ]] || (( 10#$2 > 600 )); then exit 2; fi
    elif [[ $# == 3 && "$3" != 6 && "$3" != 44 && "$3" != 149 ]]; then exit 2; fi
    if [[ "$1" == observe ]] && { [[ $# != 2 ]] || (( 10#$2 > 60 )); }; then exit 2; fi
    ${lib.optionalString (intelDriver != null) ''
      if (( 10#$2 > 120 )); then
        echo 'Driver trials are limited to 120 seconds.' >&2
        exit 2
      fi
    ''}
    if (( EUID != 0 )); then
      echo 'Administrator authentication is required to configure the Wi-Fi radio.' >&2
      exit 1
    fi
    # A system service owns the recovery journal and runs recovery even after
    # a helper crash or forced termination. Only this fixed helper is run.
    systemctl reset-failed sekka-airplay-peer.service 2>/dev/null || true
    exec systemd-run --quiet --wait --pipe --collect --unit=sekka-airplay-peer \
      --service-type=exec \
      --property="RuntimeMaxSec=$((10#$2 + 90))" \
      --property=TimeoutStopSec=45 \
      --property=KillMode=mixed \
      --property=NoNewPrivileges=yes \
      --property="ExecStartPre=${prepare}" \
      --property="ExecStopPost=${cleanup}" \
      ${airplay-radio}/bin/sekka-airplay-radio "$@"
  '';
  passthru = { inherit airplay-radio; };
}
