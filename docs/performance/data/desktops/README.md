# Data: Aizome, GNOME and Plasma in a small VM

The measurements behind [../../desktops.md](../../desktops.md), produced by
[tests/performance/desktops](../../../../tests/performance/desktops):
`bench.py` runs the VMs, `summarize.py` turns the runs into these files,
`footprint.sh` measures the closures. Subjects are named
`<desktop>-<guest RAM in MiB>`: `aizome`, `gnome`, `plasma` and
`baseline` (the same system with no desktop), at `4096` and `2048`.

| File | What it holds |
|---|---|
| `summary.json` | Per subject: medians, minimum and maximum over its boots of boot, login, memory, idle cost, latency and frame pacing, and app start-up; the biggest systemd units by PSS at the login screen and the desktop; the biggest idle wakers. The write-up's tables come from it. |
| `tables.md` | Those tables as Markdown, as pasted into the write-up. |
| `environment.json` | QEMU's version, the pinned host CPUs, one trial's full QEMU command line, and every trial in the order run, with its start and end times, host load averages and the guest clock it started with. |
| `versions.json` | Per system: kernel, Mesa, systemd, each desktop's versions, the Aizome store path, the Nixpkgs revision and the Sekka commit. |
| `footprint.json`, `footprint-raw.json` | Closure sizes (`nix path-info -S`) of the four systems without the benchmark's tools; what the three desktops add in common over the baseline, and what each adds of its own. |
| `footprint-top-<desktop>.tsv` | The 25 largest store paths each desktop adds over the baseline (MiB). |
| `trials/<subject>-<tag>/summary.json` | One boot, summarised: boot and login (host screen and guest journal), memory at the login screen and at the desktop after 60 and 190 s (MemTotal − MemAvailable, swap, zswap, PSS by group, the top units and processes), idle cost over 120 s (guest CPU, context switches, interrupts, wakeups, the busiest threads, QEMU's host CPU, PPT power), every key press's latency and the pacing statistics, and each app launch. Tags `r1`–`r3` are the 4 GiB rounds, `m1`–`m2` the 2 GiB ones. |
| `trials/.../result.json.xz` | The harness's record of that boot: host timestamps, guest clock offset, per-launch frame statistics, QMP and command-channel results. |
| `trials/.../mem-*.tsv.xz` | Per process at the login screen (`greeter`) and at the desktop (`desktop60`, `desktop190`): pid, ppid, uid, name, cgroup, RSS, PSS, PSS anon/file/shmem, swap, swap PSS (KiB), argv[0]. |
| `trials/.../meminfo-*.txt.xz`, `cgmem-*.tsv.xz` | `/proc/meminfo` with memory pressure and swap counters; each cgroup's `memory.current` and `memory.swap.current`. |
| `trials/.../idle-*.xz` | `/proc/stat` at both ends of the idle window; per thread (pid/task/tid, name, voluntary and involuntary switches, utime, stime, cgroup) at both ends; the PPT samples. |
| `trials/.../latency-*.tsv.xz` | The probe's record per press (key time, receive, commit and presentation times in the guest's CLOCK_MONOTONIC, refresh, sequence, flags, state) and the injector's press and release times (`*.inject.tsv.xz`). |
| `trials/.../pacing-*.tsv.xz` | The probe's record per frame while drawing continuously. |
| `trials/.../frames.json.xz` | Per screendump during boot and login: seconds since time zero, pixels changed against the previous frame, against the reference frame, and against the frame before the action. |
| `trials/.../journal.json.xz` | The guest's journal for that boot (`journalctl -o json`). |
| `trials/.../systemd-analyze.txt.xz`, `units.txt.xz` | `systemd-analyze`, the critical chain and `blame`; the display manager's and targets' activation times. |
| `trials-virtio-gpu-pci/` | The three boots made with `virtio-gpu-pci` instead of `virtio-vga` before the switch (not in the results): their summaries, and GDM's journal lines showing its 10 s wait for a primary GPU. |
| `screens/` | Frames as the harness saw them, at their original 1920×1080 (PNG, losslessly recompressed): each desktop's login screen and desktop as the reference frames, Aizome's and GNOME's first login, the no-desktop console. Fixture user only. |
