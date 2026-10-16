# Hand-off: the Network VM test's hidden-network join

2026-10-16, America/Chicago. Branch `claude/competent-napier-1fca3e`, from
`main` at `bd17242`. **Work in progress: do not merge.** Delete `docs/handoff/`
before this branch is merged.

## The task

`nix flake check` runs `checks.x86_64-linux.network-vm`
(`nix/network-vm-test.nix`) before the nightly workflow publishes the binary
cache, moves `nightly` and publishes the ISO, so one flaky VM test blocks
every user's update. Its subtest "Joining a hidden network by name" fails
intermittently: CI nightly run 38959767706 attempt 1 on `bd17242`, and one
local run before this work. The goal is to make it reliable while it still
checks that the Network window creates and activates a hidden connection by
name. Simply raising timeouts or retrying until it passes is out of bounds.
Done means at least 5 consecutive passes, some while the machine is busy,
and a PR that is not merged.

In every failure the window did its part. The connection exists with
`802-11-wireless.hidden=yes`, NetworkManager starts activating it, and 25
seconds later it logs "Activation: (wifi) association took too long, failing
activation". The window then removes the connection (a failed join leaves
nothing behind), and NetworkManager rejoins Home.

## Status

- **Root cause found** (below). It is a race between NetworkManager's
  scan-address randomization and wpa_supplicant's scan bookkeeping, not slow
  radio discovery.
- **Partial fix committed:** `networking.networkmanager.wifi.scanRandMacAddress = false`
  in `nix/simulated-wifi.nix`. This removes the race, but it exposes a second
  problem: every full scan of the simulated radio takes about 9.5 s, and each
  join now needs two of them back to back. In the one run with this change, the
  hidden join passed in 16.6 s, but the last subtest failed (see "What was
  run").
- **Not done:** making scans short (next steps), the 5+ consecutive passes
  under load, and removing this directory.

## Root cause

The sources are NetworkManager 1.58.1 and wpa_supplicant 2.12, as used in the
test VM (kernel 6.18.56). Line numbers refer to those release tarballs.

1. **NetworkManager bounces wlan1 twice while it switches networks.** The
   window runs `nmcli connection up` for "Back Room" while Home is active.
   The device goes activated → deactivating → disconnected → prepare. On
   entering *disconnected*, `_scan_notify_allowed()` kicks off a periodic
   scan that probes the hidden profile's SSID
   (`src/core/devices/wifi/nm-device-wifi.c` ~503–559, `_scan_kickoff()`
   ~1758). Just before it sends the request, `_hw_addr_set_scanning()` (~1413,
   called at ~1914) gives wlan1 a random MAC address, because
   `wifi.scan-rand-mac-address` defaults to yes. `_hw_addr_set()`
   (`src/core/devices/nm-device.c` ~18628) always takes a Wi-Fi device down to
   change its address. Activation then starts, and stage 1 sets the address
   back (`wifi.cloned-mac-address=preserve`), which takes wlan1 down and up a
   second time.
2. **wpa_supplicant marks NetworkManager's scan as scan-only.** NetworkManager
   requests every scan with `AllowRoam=false`
   (`src/core/supplicant/nm-supplicant-interface.c` ~2826). wpa_supplicant then
   sets `wpa_s->scan_res_handler = scan_only_handler`
   (`wpa_supplicant/dbus/dbus_new_handlers.c` ~1779) and queues the scan as
   radio work.
3. **If that work is dropped before it starts, the mark stays.** When wlan1
   goes down, wpa_supplicant removes queued radio work.
   `wpas_trigger_scan_cb(work, deinit=1)` with `!work->started` only frees the
   parameters (`wpa_supplicant/scan.c` ~201–205). The handler is cleared only
   when results arrive (`events.c` ~2605–2614) or when starting the scan fails
   at once (`scan.c` ~246–273, without a retry for a manual scan).
4. **The scan that finds the hidden network is then ignored.** After
   `SelectNetwork`, wpa_supplicant runs its own scan with the hidden SSID
   (`wpa_supplicant.c` ~5705–5747). It finds `02:00:00:00:00:01 'Back Room'`,
   but the stale handler takes the results: the log says "Scan-only results
   received", and wpa_supplicant neither associates nor schedules another
   scan. NetworkManager allows no scans of its own during prepare/config, so
   nothing happens until its supplicant timeout. That limit is 25 s
   (`SUPPLICANT_DEFAULT_TIMEOUT`, `nm-device.c` ~19167), and the device fails
   with reason `ssid-not-found`.
5. **Pass or fail depends on interleaving.** If wpa_supplicant starts the
   queued scan while wlan1 is down, the trigger fails with ENETDOWN
   (`CTRL-EVENT-SCAN-FAILED ret=-100`) and clears the handler. Its own scan
   then joins about 9.5 s later. That is what every passing run shows. If the
   work is still queued when the second bounce removes it, the join fails. The
   CI failure has no `SCAN-FAILED` line, which fits the failing path. Load
   changes the interleaving, which is why CI, where all VM tests run in
   parallel, fails more often.

`docs/handoff/network-vm-hidden-ssid-evidence.log` has the filtered log
lines for each case.

### Scan duration, the second factor

mac80211_hwsim in 6.18 offers 89 channels: 14 at 2.4 GHz, 24 at 5 GHz and 51
S1G channels at 902–927 MHz (`NUM_S1G_CHANS_US`,
`drivers/net/wireless/virtual/mac80211_hwsim.c` ~490 and ~520 at v6.18.56).
Under the world regulatory domain most of them are passive. One full scan
takes about 9.5 s ("Radio work 'scan' done in 9.6 seconds"), and one scan in
the debug run took 26 s. It is not clear why the S1G channels are enabled
under the world domain, whose rules do not cover them.

### Not only a test problem (unverified on hardware)

Sekka does not override `networking.networkmanager.wifi.scanRandMacAddress`
(true by default), and installed systems run the same NetworkManager and
wpa_supplicant. Joining a hidden network while connected to another could
therefore hit the same race on real hardware. Upstream hostap master (w1.fi
cgit, fetched 2026-10-16) still has the same `wpas_trigger_scan_cb` deinit
path. Both points are follow-ups, not part of this task: an upstream
wpa_supplicant report (clear `scan_res_handler` when a scan's radio work is
removed before it starts, or on `INTERFACE_DISABLED`), and a product decision
for installed systems.

## What was run

All runs used `nix build .#checks.x86_64-linux.network-vm.driver`, then
`result/bin/nixos-test-driver -o <existing dir>`, outside the build sandbox on
a 16-core, 60 GiB host. Each was a single disposable VM.

| Run | Change | Result |
| --- | --- | --- |
| Baseline | none | Passed. Hidden join waited 13.4 s; `SCAN-FAILED ret=-100` path. |
| Diagnostic | debug logging (patch below) | **Failed**: reproduced the stale scan-only handler; journal captured. |
| A | `scanRandMacAddress = false` + debug logging | Hidden join passed (16.6 s wait after a 3 s sleep). Home join took 18.7 s instead of about 4 s. **Final subtest failed**: `"Back Room" in active()` was false, because NetworkManager had not yet reactivated Back Room after the wrong-password attempt. |

With A, each switch runs NetworkManager's periodic scan at the brief
*disconnected* state (~9.5 s). wpa_supplicant's `SelectNetwork` scan waits
behind it ("Already scanning - Reschedule the incoming scan req" every
second), then does its own ~9.5 s scan before it associates. That is about
19.4 s from stage 2 to association against NetworkManager's 25 s limit: too
little margin under CI load. The previous passing runs were faster only
because the race usually aborted NetworkManager's scan.

## Next steps

1. **Keep the `scanRandMacAddress = false` change.** It removes the root cause
   in the VM.
2. **Make a full scan of the simulated radio short**, so two scans fit easily
   in 25 s and NetworkManager reconnects promptly. Candidates, cheapest to
   check first:
   - First apply the diagnostics patch and run its `probe()` block, which has
     not been run yet. It prints `iw reg get`, the radio's channel list and the
     time of an `iw dev wlan1 scan` under the world domain, then after
     `iw reg set US` and `iw reg set JP`. Then remove the
     `raise Exception("probe done")` line and the probe block.
   - `boot.extraModprobeConfig = "options mac80211_hwsim regtest=5"`
     (`HWSIM_REGTEST_CUSTOM_WORLD`), which gives every radio
     `hwsim_world_regdom_custom_03`: 2.4 GHz channels 1–11 and 14, 5 GHz
     36–48 and 149–165, 5.9 GHz and 6 GHz 5955–7125, with no NO-IR flags
     (hwsim source ~184 and ~7124). The expected result is S1G and DFS
     channels disabled and every remaining channel scanned actively. 6 GHz may
     add channels, so measure it. hostapd uses channel 6, which stays allowed.
   - `iw reg set <country>` in `start_access_points`. The wireless-regdb is
     installed through the wpa_supplicant module.
   - `freq_list=2437` for wpa_supplicant through
     `networking.wireless.extraConfigFiles`. Do not use `extraConfig`: it
     switches wpa_supplicant from `imperative.conf` to `nixos.conf`, which also
     adds `pmf=1` and a bgscan line. This shortens only wpa_supplicant's own
     scans (`scan.c` ~1425). NetworkManager's D-Bus scans pass no channels
     and stay full length, so a join would take about 10 s instead of 19 s.
     It is a fallback if the channel list cannot be reduced.
   - Whatever is chosen, derive the frequency or channel from the `channel=6`
     in `simulated-wifi.nix` rather than repeating 2437, and explain it in a
     comment.
3. **Recheck the final subtest.** `assert "Back Room" in active()` assumes
   NetworkManager reactivates Back Room right after Office is removed. With
   short scans it should again. If it still races, replacing it with a wait
   for Back Room to become active is a legitimate wait for autoconnect, but
   only once scans are short. Otherwise it hides the slowness.
4. **Verify:** at least 5 consecutive passes of the unmodified test (no
   diagnostics), some while the host is busy. For example, run 2–3 drivers in
   parallel, each with its own output directory (each VM uses 4 cores and
   4 GiB), or run alongside `nix flake check`. Record the hidden join's wait
   time in each run.
5. **Before merge:** remove `docs/handoff/`, rewrite the `simulated-wifi.nix`
   comment so it no longer points here, and update the PR description. Leave
   the release workflow inhibited.

Do not raise NetworkManager's supplicant timeout (the global
`802-1x.auth-timeout` default feeds `nm_device_get_supplicant_timeout()`), and
do not retry the join. Both are out of bounds for this task.

## Reproducing and diagnosing

```sh
nix build .#checks.x86_64-linux.network-vm.driver --out-link /tmp/nvm/driver
mkdir -p /tmp/nvm/out
/tmp/nvm/driver/bin/nixos-test-driver -o /tmp/nvm/out
```

`-o` must name an existing directory. For debug logs, apply
`docs/handoff/network-vm-hidden-ssid-diagnostics.patch` (`git apply`). It sets
wpa_supplicant's D-Bus `DebugLevel` to `msgdump` with timestamps and
NetworkManager's WIFI, WIFI_SCAN, SUPPLICANT, DEVICE, CORE and PLATFORM
logging to TRACE before the hidden subtest. It also copies the whole journal
to `<out>/journal.txt` when that subtest ends, pass or fail. It also contains
the unrun `probe()` block described above; delete the block if you do not
want it. Starting hostapd with `-t -dd` adds its debug log, but
`copy_from_machine("/tmp/hostapd.log")` found no file there; this was not
investigated. Useful journal searches: `Scan-only results`,
`Radio work 'scan'`, `set-hw-addr`, `Interface (down|up)`,
`CTRL-EVENT-SCAN-FAILED`, `Already scanning`.

To read the failing CI log:
`gh run view 38959767706 --attempt 1 --log-failed`.
