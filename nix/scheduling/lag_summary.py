"""Bounded, queryable projection of lag-recorder samples, without new measurements.

Names and units match the sampler: memory is bytes, PSI averages/stall are
percent, vmstat rates are kernel counter units/second (pswpin/out are pages),
and cgroup statistics remain cumulative counters. Successful read envelopes
are retained, as are failure details and explicit None interval values. Missing
fields stay absent, never become zero. Configured memory.low/min plus ancestor
usage are evidence, not a claim about effective protection.

All selected cgroups retain configured protection, limits and occupancy. Only
session/compositor, background and nix-daemon groups retain fault/event counters
and avg10/interval PSI; global PSI retains all three average windows. Detailed
ancestor counters and longer cgroup PSI windows remain in full snapshots.
"""

from copy import deepcopy


MAX_GROUPS = 16
MAX_DEVICES = 8
MAX_PROCESSES = 8
MAX_ERRORS = 16
READ_FIELDS = ("status", "error", "errno", "message")
IDENTITY_FIELDS = (
    "schema", "run_id", "boot_id", "sample_id", "wall_utc", "monotonic_seconds",
    "elapsed_seconds", "sampling_gap_seconds", "schedule_lateness_seconds",
    "sample_duration_seconds", "worker_busy", "event_id", "context",
)
PSI_FIELDS = ("avg10", "avg60", "avg300", "stall_percent", "interval_status")
CPU_FIELDS = ("user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal")
MEM_FIELDS = (
    "MemTotal", "MemAvailable", "MemFree", "Cached", "SwapTotal", "SwapFree",
    "SwapCached", "Zswap", "Zswapped", "Dirty", "Writeback",
)
VM_FIELDS = (
    "pgfault", "pgmajfault", "pswpin", "pswpout", "oom_kill",
    "zswpin", "zswpout", "zswpwb", "compact_stall",
    "pgscan_direct", "pgscan_kswapd", "pgsteal_direct", "pgsteal_kswapd",
    "allocstall_normal", "allocstall_movable",
    "workingset_refault_anon", "workingset_refault_file",
)
GROUP_MEMORY_FIELDS = (
    "memory.current", "memory.swap.current", "memory.zswap.current",
    "memory.low", "memory.min", "memory.high", "memory.max",
    "memory.swap.high", "memory.swap.max", "memory.zswap.max",
)
GROUP_STAT_FIELDS = (
    "pgmajfault", "workingset_refault_anon", "workingset_refault_file",
    "pgscan_direct", "zswpin", "zswpout", "zswpwb",
)
GROUP_EVENT_FIELDS = ("low", "high", "max", "oom", "oom_kill", "oom_group_kill", "sock_throttled")
DISK_COUNTER_FIELDS = (
    "reads", "reads_merged", "sectors_read", "read_ms", "writes", "writes_merged",
    "sectors_written", "write_ms", "io_ms", "weighted_io_ms", "discards",
    "discards_merged", "sectors_discarded", "discard_ms", "flushes", "flush_ms",
)
DISK_INTERVAL_FIELDS = (
    "read_bytes_per_second", "write_bytes_per_second", "busy_percent",
    "read_await_ms", "write_await_ms", "discard_await_ms", "flush_await_ms", "await_ms",
)
PROCESS_COUNTER_FIELDS = ("cpu_ticks", "minor_faults", "major_faults")
DETAILED_GROUPS = frozenset((
    "aizome.service", "session.slice", "app-background.slice", "background.slice",
    "nix-daemon.service",
))


def _select(source, fields):
    return {key: deepcopy(source[key]) for key in fields if key in source}


def _bounded_list(source, target, key, limit, project=deepcopy):
    if key not in source:
        return
    target[key] = [project(item) for item in source[key][:limit]]
    omitted = len(source[key]) - len(target[key])
    if omitted:
        target[key + "_truncated"] = True
        target[key + "_omitted"] = omitted


def _reading(source, fields=None):
    result = _select(source, READ_FIELDS)
    if "value" in source:
        result["value"] = (deepcopy(source["value"]) if fields is None
                           else _select(source["value"], fields))
    _bounded_list(source, result, "not_reported", 64)
    return result


def _pressure(source, fields=PSI_FIELDS):
    result = _select(source, READ_FIELDS)
    if "value" in source:
        result["value"] = {scope: _select(values, fields)
                           for scope, values in source["value"].items()
                           if scope in ("some", "full")}
    return result


def _interval(source, fields):
    result = {}
    for key in ("per_second", "unavailable"):
        if key in source:
            result[key] = _select(source[key], fields)
    return result


def _group(path, source):
    result = _select(source, (*READ_FIELDS, "inode"))
    for key in GROUP_MEMORY_FIELDS:
        if key in source:
            result[key] = _reading(source[key])
    if path.rsplit("/", 1)[-1] in DETAILED_GROUPS:
        for key, fields in (("memory.stat", GROUP_STAT_FIELDS),
                            ("memory.events", GROUP_EVENT_FIELDS)):
            if key in source:
                result[key] = _reading(source[key], fields)
        for key in ("cpu.pressure", "memory.pressure", "io.pressure"):
            if key in source:
                result[key] = _pressure(source[key], ("avg10", "stall_percent", "interval_status"))
    return result


def _group_priority(path):
    # Keep the monitored desktop and its protection ancestors ahead of unrelated
    # recorder ancestors if the source's 32-group cap exceeds our timeline cap.
    leaf = path.rsplit("/", 1)[-1]
    if leaf in ("aizome.service", "session.slice"):
        return 0
    if path in ("/", "/system.slice", "/system.slice/nix-daemon.service"):
        return 1
    if leaf in ("app-background.slice", "background.slice", "user.slice"):
        return 1
    if ((leaf.startswith("user-") and leaf.endswith(".slice"))
            or (leaf.startswith("user@") and leaf.endswith(".service"))):
        return 1
    if leaf == "app.slice":
        return 2
    return 3


def _disk(source):
    result = _select(source, (*READ_FIELDS, "sysfs_inode"))
    if "counters" in source:
        result["counters"] = _select(source["counters"], ("in_flight",))
    if "interval" in source:
        result["interval"] = _interval(source["interval"], DISK_COUNTER_FIELDS)
        if "per_second" in source["interval"]:
            result["interval"]["per_second"] = _select(
                source["interval"]["per_second"], ("reads", "writes", "discards", "flushes"))
        result["interval"].update(_select(source["interval"], DISK_INTERVAL_FIELDS))
    return result


def _process(source):
    result = _select(source, (*READ_FIELDS, "pid", "start_ticks", "comm", "state", "rss_bytes"))
    for key, fields in (("memory_status", ("VmSwap",)),
                        ("schedstat", ("runqueue_wait_ns",))):
        if key in source:
            result[key] = _reading(source[key], fields)
    if "wchan" in source:
        result["wchan"] = _reading(source["wchan"])
    if "interval" in source:
        result["interval"] = _interval(source["interval"], PROCESS_COUNTER_FIELDS)
        result["interval"].update(_select(source["interval"], ("cpu_percent_one_core", "identity_status")))
    return result


def _ipc(source):
    result = _select(source, (*READ_FIELDS, "latency_seconds", "peer_pid", "socket"))
    _bounded_list(source, result, "attempts", 16,
                  lambda item: _select(item, (*READ_FIELDS, "latency_seconds", "peer_pid", "socket")))
    return result


def compact_sample(sample):
    """Return an independent summary; never infer measurements or identities.

    Every collection cap reports the omitted count and a truncation flag.
    Existing collector truncation/errors remain separate from summary caps.
    Full samples hold excluded cumulative totals, I/O cgroup device tables,
    derived ancestor ceilings, executable paths and compositor build metadata.
    """
    result = {"type": "summary", **_select(sample, IDENTITY_FIELDS)}
    for key in ("reasons", "trigger_reasons"):
        _bounded_list(sample, result, key, 32)
    if "pressure" in sample:
        result["pressure"] = {name: _pressure(sample["pressure"][name])
                              for name in ("cpu", "memory", "io") if name in sample["pressure"]}
    if "cpu" in sample:
        result["cpu"] = _select(sample["cpu"], READ_FIELDS)
        if "percent" in sample["cpu"]:
            result["cpu"]["percent"] = _select(sample["cpu"]["percent"], CPU_FIELDS)
    for key, fields in (("loadavg", ("one", "five", "fifteen", "runnable", "tasks")),
                        ("meminfo_bytes", MEM_FIELDS)):
        if key in sample:
            result[key] = _reading(sample[key], fields)
    if "vmstat" in sample:
        source = sample["vmstat"]
        result["vmstat"] = _select(source, READ_FIELDS)
        _bounded_list(source, result["vmstat"], "not_reported", 64)
        if "interval" in source:
            result["vmstat"]["interval"] = _interval(source["interval"], VM_FIELDS)
    if "diskstats" in sample:
        source = sample["diskstats"]
        disks = _select(source, (*READ_FIELDS, "truncated_devices"))
        if "devices" in source:
            entries = list(source["devices"].items())
            disks["devices"] = {name: _disk(value) for name, value in entries[:MAX_DEVICES]}
            if len(entries) > MAX_DEVICES:
                disks["summary_truncated_devices"] = True
                disks["summary_omitted_devices"] = len(entries) - MAX_DEVICES
        _bounded_list(source, disks, "unavailable_devices", MAX_ERRORS,
                      lambda item: _select(item, (*READ_FIELDS, "device")))
        result["diskstats"] = disks
    if "cgroups" in sample:
        entries = sorted(sample["cgroups"].items(), key=lambda item: _group_priority(item[0]))
        result["cgroups"] = {path: _group(path, group) for path, group in entries[:MAX_GROUPS]}
        if len(entries) > MAX_GROUPS:
            result["cgroups_truncated"] = True
            result["cgroups_omitted"] = len(entries) - MAX_GROUPS
    if "aizome" in sample:
        source = sample["aizome"]
        desktop = _select(source, (*READ_FIELDS, "bounded_scan"))
        if "processes" in source:
            processes = sorted(source["processes"], key=lambda item: item.get("comm") != "aizome")
            _bounded_list({"processes": processes}, desktop, "processes", MAX_PROCESSES, _process)
        _bounded_list(source, desktop, "errors", MAX_ERRORS,
                      lambda item: _select(item, (*READ_FIELDS, "path", "pid", "start_ticks")))
        result["aizome"] = desktop
    if "compositor_ipc" in sample:
        result["compositor_ipc"] = _ipc(sample["compositor_ipc"])
    return result
