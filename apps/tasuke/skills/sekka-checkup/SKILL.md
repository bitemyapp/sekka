---
name: sekka-checkup
description: "A read-only health check of this computer and how to report it — failed services, errors since boot, disk and Nix store space, memory and swap, CPU hogs, the last system update, battery health, network and DNS, clock sync, crashes. Load for \"give my computer a check-up\", \"is everything OK?\", \"why is it slow?\" (as a first pass), \"anything I should fix?\"."
---

# Checking up on Sekka

A check-up only looks: every command below is read-only and runs as the
person with `run_command`, so none of it asks for approval. Run them,
then report. Don't fix anything during the check-up; offer fixes at the
end, and make each one only after the person says yes.

## What to run

Run these, a few per call where they are independent (join with `;`
only when each part is read-only). A command that isn't installed is
not a problem: skip it.

| Area | Command | Worth mentioning when |
|---|---|---|
| Failed services | `systemctl --failed --no-pager` and `systemctl --user --failed --no-pager` | any unit is listed |
| Errors since boot | `journalctl -b -p err --no-pager -n 40` | the same error repeats, or a service keeps failing; one-off hardware probe noise at boot is normal |
| Disk space | `df -h / /home /nix 2>/dev/null` | a filesystem is over 85% full |
| Journal size | `journalctl --disk-usage` | over 1 GB |
| System generations | `ls -d /nix/var/nix/profiles/system-*-link` (count the lines) | more than about 20 (each keeps its packages on disk) |
| Last system update | `stat -c %y /nix/var/nix/profiles/system` (when the newest system was made; a live USB session or a test machine has no system profile: skip this and the line above) | the newest generation is more than 2 weeks old |
| Memory and swap | `free -h` and `swapon --show` | available memory under 10%, or swap mostly used |
| Busy processes | `uptime`, `nproc`, and the command below the table | something uses a whole core with nothing open that explains it, or load stays above the core count (`nproc`) |
| Crashes | `coredumpctl list --since=-7d --no-pager` | an app crashed more than once |
| Network | `nmcli general status` and `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device` | not connected, or "limited" connectivity |
| Name lookup | `getent hosts nixos.org` | no answer while the network is connected |
| Clock | `timedatectl status` | "System clock synchronized: no" for long |
| Battery (laptops) | `cat /sys/class/power_supply/BAT*/capacity /sys/class/power_supply/BAT*/status` and `cat /sys/class/power_supply/BAT*/energy_full /sys/class/power_supply/BAT*/energy_full_design` | health (full ÷ design) under 80% |
| Graphics (NVIDIA) | `nvidia-smi --query-gpu=name,temperature.gpu,memory.used,memory.total --format=csv` | over 85 °C idle |

The busiest processes (a pipe, so it is outside the table):

```sh
ps -eo pid,user,comm,%cpu,%mem --sort=-%cpu | head -6
```

Disk health (SMART) and some logs need root; don't ask for a password
during a check-up. Mention that a deeper check can be run as
administrator if the person wants it.

## How to report

Lead with the verdict in one sentence ("Your computer looks healthy" or
"Two things need attention"). Then one line per area, in this order:
problems first, then warnings, then a single line listing what is fine.
Use ✓ for fine, ⚠ for worth attention, ✕ for a problem, and give the
number that matters (87% full, 3 failed units, updated 23 days ago).

Finish with at most three suggestions, most useful first, each with what
it would do and whether it needs the administrator password, and ask
which to do. The usual fixes are in the sekka-maintenance skill (disk
space, updating, services) and sekka-troubleshooting (failed services,
repeated errors): load the one you need before acting.

Keep the whole report short enough to read at a glance. Don't paste raw
command output into it.
