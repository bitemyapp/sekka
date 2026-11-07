#!/usr/bin/env python3
"""Reproducible Bazel/Buck2 qualification, with immutable observations.

plan prints the complete scenario graph; run executes a JSON configuration in a
new owned root, or resumes the same configuration without overwriting trials.
Setup, artifact materialization, correctness probes and storage accounting are
recorded separately from timed builds. Commands are argv arrays, never shell
fragments. A failed/interrupted cold trial remains cold; resume never replaces it.

Configuration keys: tool, repo, commit (full frozen SHA), root, client (argv),
environment (explicit overrides), profiles (dev/release/release-lto, each with
features and targets), artifacts (leaf/shared/icon selectors), setup (argv lists),
service (identity, inventory/metrics/restart/eviction/restore_capacity/dedupe argv and APIs),
storage (roots, immutable_roots, command, optional immutable_command, attribution_command
and max_active_union_bytes),
test_groups (nine named argv suffixes), soroban_tests, correctness_behavior
(argv producing the eleven real libtest results), startup (private headless argv),
startup_result (test-startup.py result.json path; exact executables and PNGs checked),
runtime_artifacts (Bazel runtime placeholder -> artifact selector key) or
runtime_targets (Buck runtime placeholder -> target name; ergonomic runtime API),
nix_identity (evidence paths), variants (client/service/source identity metadata),
plan (full, the default, or storage: one observation per storage/reuse/correctness
scenario, edit latency reported but not gated, no churn, release-LTO or startup).
Artifact selectors have target, optional file_regex, output_group and member
(e.g. icons.rs); selectors may be keyed by profile. Runtime artifact placeholders
include compositor/shell/ctl/koe. The compositor also runs --build-info.
Bitcode-only rlibs require bitcode_decoder (argv with {member}/{decoded}, e.g.
matching clang -S -emit-llvm -x ir {member} -o {decoded}); inspection decodes the
actual archive members outside build timing and preserves inputs/IR/command logs.
Artifact checks require their own current marker and allow current dependency
markers embedded by linking; every stale/unexpected marker still fails.

Buck configuration requires buck_engine (native argv), buck_telemetry_flags
including --buck-arg=--build-report={telemetry}.report.json and
--buck-arg=--event-log={telemetry}.events.json-lines.gz. Keep buck_isolation
identical across clients: it enters action paths. Output-loss recovery stops only
the owned daemon and quarantines mutable output state, preserving its original
action path and all cache data. Quarantine is evidence, not active workload.
Buck telemetry adds --write-build-id and binds the final trace to that file.
Every Build ID in the exact invocation log is retained before postprocessing;
native retries can leave --event-log pointing at an earlier failed attempt.
Compiler totals cover all attempts, remain unknown if any failed compiler action
could have stopped before rustc, and expose final-attempt counts separately.
Buck libtest acceptance requires native executed-test records from log what-ran
against retained exact-attempt events, never an implicit latest log.
Native test.run records retain their executor identity: real executed tests may
run remotely, and do not by themselves prove local runtime or manager coverage.
Libtest counts are retained even for failed test commands, never only successes.

Storage command must emit compsize's compression-aware TOTAL row and receives
ALL selected roots in ONE invocation. Roots may use glob:PATH, or untracked:GLOB for
the paths git ignores or does not track in each matching checkout (its build state,
leaving tracked source out of the sharing figure); unmatched roots
fail. Separate active/immutable unions are never summed. The sharing denominator
is the one-tree post-edit/revert workload, not the smaller original cold cache.
Storage roots are checked by Btrfs filesystem UUID, not st_dev: separate Btrfs
subvolumes of one filesystem legitimately have different device numbers.
max_active_union_bytes is an observed churn-budget threshold, not an enforcement
mechanism. Automatic total-state boundedness remains unmeasured without separate
mechanism and enforcement evidence covering all owned state.
Optional cargo_baseline JSON supplies commit, features, leaf/shared/icon median
seconds; differing commits additionally require equal comparison_identity.
Completed command timings remain in summaries even when trial postprocessing or
later stage storage fails. A missing trial result is explicitly failed/incomplete,
never a successful observation or a reason to replay its timing.
Credentials belong in private client rc/manifest files, not literal argv tokens.
Placeholders: {tree}, {root}, {profile}, {jobs}, {trial}, {state}, {output},
{python}; these are literal substitutions, never shell expansions.

Examples: shared-build-bench.py plan --kind storage
          shared-build-bench.py smoke config.json --kind shared --edit
          shared-build-bench.py run config.json --go
          shared-build-bench.py summarize /owned/run/root
"""
import argparse
import concurrent.futures
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import statistics
import subprocess
import sys
import threading
import time
import traceback
import uuid

SCHEMA = 1
OWNER = '.sekka-shared-build-bench.json'
LEAF = 'desktop/aizome/apps/soroban/src/main.rs'
SHARED = 'desktop/aizome/crates/aizome-theme/src/lib.rs'
ICONS = 'desktop/aizome/crates/aizome-kit/icons/lucide'
PROFILES = ('dev', 'release', 'release-lto')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def btrfs_filesystem_id(path):
    """BTRFS_IOC_FS_INFO uses a 1024-byte struct with its 16-byte fsid at offset 16."""
    fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC)
    try:
        info = bytearray(1024)
        fcntl.ioctl(fd, 0x8400941f, info, True)
        return bytes(info[16:32]).hex()
    finally:
        os.close(fd)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def save(path, value):
    """Atomically publish create-only observations; retain pending writes on failure."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name('.' + path.name + '.pending-' + uuid.uuid4().hex)
    with temporary.open('x') as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.link(temporary, path)  # Atomic publication, refusing an existing observation.
    temporary.unlink()
    directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def load(path):
    return json.loads(Path(path).read_text())


def plan(selection='full'):
    stages = []
    def add(name, trees=('a',), profile='dev', jobs=16, edits=None, operation='build', **extra):
        stages.append(dict(id=name, trees=list(trees), profile=profile, jobs=jobs,
                           edits=edits or {}, operation=operation, **extra))
    add('dev-cold', storage=True)
    for n in range(1, 6):
        add(f'dev-noop-{n}')
    for kind in ('leaf', 'shared', 'icon'):
        for n in range(1, 6):
            add(f'dev-{kind}-{n}', edits={kind: f'{kind}-{n}'})
    add('dev-revert', reference='dev-cold', storage=True)
    for tree in 'bcde':
        add(f'dev-fresh-{tree}', trees=(tree,), reference='dev-cold', storage=True)
    add('dev-divergent-j4', trees=tuple('abcde'), jobs=4,
        edits={'leaf': 'divergent-{tree}-leaf', 'shared': 'divergent-{tree}-shared'}, storage=True)
    add('dev-identical-new-j8', trees=('a', 'b'), jobs=8, edits={'shared': 'same-new-edit'})
    for jobs in (4, 16, 32):
        for n in range(1, 4):
            add(f'dev-shared-j{jobs}-{n}', jobs=jobs, edits={'shared': f'jobs-{jobs}-{n}'})
    for n in range(1, 11):
        kind = 'shared' if n % 2 else 'icon'
        add(f'dev-churn-{n}', edits={kind: f'churn-{n}'}, storage=True)
    add('dev-post-churn-original', reference='dev-cold', storage=True)
    add('dev-post-churn-known', edits={'shared': 'shared-1'})
    add('service-eviction', operation='eviction', storage=True)
    add('dev-post-eviction-original', reference='dev-cold', output_loss=True)
    add('dev-post-eviction-known', edits={'shared': 'shared-1'})
    add('service-restart', operation='restart')
    add('dev-restart-edited', edits={'shared': 'shared-1'})
    add('dev-restart-original', reference='dev-cold')
    add('dev-primed-output-loss', reference='dev-cold', storage=True, output_loss=True)
    add('service-restore-capacity', operation='restore_capacity')
    for profile in ('release', 'release-lto'):
        add(f'{profile}-first', profile=profile)
        add(f'{profile}-noop', profile=profile)
        add(f'{profile}-edit', profile=profile, edits={'shared': f'{profile}-edit'})
        add(f'{profile}-revert', profile=profile, reference=f'{profile}-first')
        add(f'{profile}-fresh', trees=(f'{profile}-fresh',), profile=profile,
            reference=f'{profile}-first', storage=True)
    for n in range(9):
        add(f'test-group-{n + 1}', operation='test', test_group=n)
    add('soroban-11', operation='soroban', expected_tests=11)
    for tree in 'bcde':
        add(f'soroban-reuse-{tree}', trees=(tree,), operation='soroban', expected_tests=11)
    for profile in PROFILES:
        add(f'{profile}-private-headless-startup', profile=profile, operation='startup')
    add('service-dedupe', operation='dedupe', storage=True)
    add('final-original', reference='dev-cold', storage=True)
    if selection == 'storage':
        stages = [stage for stage in stages if stage['id'] in STORAGE_PLAN]
    return stages


# Storage-first qualification: one observation per scenario, the full plan's
# definitions and order. Latency medians, job sweeps and churn are left out.
STORAGE_PLAN = frozenset(
    ['dev-cold', 'dev-noop-1', 'dev-leaf-1', 'dev-shared-1', 'dev-icon-1', 'dev-revert']
    + [f'dev-fresh-{tree}' for tree in 'bcde']
    + ['dev-divergent-j4', 'service-eviction', 'dev-post-eviction-original', 'service-restart',
       'dev-restart-original', 'dev-primed-output-loss', 'service-restore-capacity',
       'release-first', 'release-fresh', 'soroban-11', 'service-dedupe', 'final-original']
    + [f'test-group-{n}' for n in range(1, 10)])
PLANS = ('full', 'storage')


def expand(argv, values):
    result = []
    for arg in argv:
        if not isinstance(arg, str):
            raise ValueError('command arguments must be strings')
        for key, value in values.items():
            arg = arg.replace('{' + key + '}', str(value))
        result.append(arg)
    return result


def json_stream(path):
    """Decode JSONL or concatenated pretty objects without retaining giant logs."""
    decoder = json.JSONDecoder()
    text = ''
    eof = False
    with Path(path).open() as stream:
        while True:
            text = text.lstrip()
            if not text and eof:
                return
            try:
                value, end = decoder.raw_decode(text)
            except json.JSONDecodeError:
                if eof:
                    raise
                chunk = stream.read(1024 * 1024)
                if chunk:
                    text += chunk
                else:
                    eof = True
                continue
            yield value
            text = text[end:]


def bazel_telemetry(prefix):
    result = {'native_files': {}, 'compiler_executions': None}
    bep = Path(str(prefix) + '.bep.json')
    execution = Path(str(prefix) + '.exec.json')
    for name, path in [('bep', bep), ('execution', execution)]:
        result['native_files'][name] = {'path': str(path), 'exists': path.exists()}
    if bep.exists():
        for event in json_stream(bep):
            if 'buildMetrics' in event:
                result['build_metrics'] = event['buildMetrics']
            if 'testSummary' in event:
                result.setdefault('test_summaries', []).append(event['testSummary'])
    if execution.exists():
        actions, runners, hits, keys = {}, {}, 0, []
        memory_peaks = []
        memory_unmeasured = 0
        for action in json_stream(execution):
            runner = action.get('runner', 'unknown')
            runners[runner] = runners.get(runner, 0) + 1
            if action.get('cacheHit'):
                hits += 1
            else:
                mnemonic = action.get('mnemonic', 'unknown')
                actions[mnemonic] = actions.get(mnemonic, 0) + 1
                peak = action.get('metrics', {}).get('measuredMemoryPeakBytes')
                if peak is not None and int(peak) > 0:
                    memory_peaks.append(int(peak))
                else:
                    memory_unmeasured += 1
            keys.append({k: action[k] for k in ('digest', 'mnemonic', 'targetLabel', 'runner', 'cacheHit', 'metrics') if k in action})
        result.update(executed_mnemonics=actions, runners=runners, remote_or_disk_hits=hits,
                      compiler_executions=sum(n for key, n in actions.items() if 'Rustc' in key or key == 'CppCompile'),
                      action_keys=keys)
        result['maximum_executed_action_memory_bytes'] = max(memory_peaks) if memory_peaks else None
        result['memory_sample_count'] = len(memory_peaks)
        result['memory_unmeasured_action_count'] = memory_unmeasured
        result['memory_method'] = 'largest positive native measuredMemoryPeakBytes, not concurrent RSS; zero/missing defaults unmeasured; cache-hit metadata excluded'
    result['action_cpu_seconds'] = None
    result['action_cpu_reason'] = 'not inferred from client CPU or summed wall time; native raw telemetry retained'
    cpu_by_mnemonic = {}
    for action in result.get('build_metrics', {}).get('actionSummary', {}).get('actionData', []):
        mnemonic = action.get('mnemonic')
        if mnemonic in result.get('executed_mnemonics', {}) and ('userTime' in action or 'systemTime' in action):
            cpu_by_mnemonic[mnemonic] = sum(float(action.get(key, '0s').removesuffix('s'))
                                            for key in ('userTime', 'systemTime'))
    result['action_cpu_seconds_by_mnemonic'] = cpu_by_mnemonic
    result['action_cpu_seconds_reported_sum'] = sum(cpu_by_mnemonic.values()) if cpu_by_mnemonic else None
    if set(result.get('executed_mnemonics', {})) == set(cpu_by_mnemonic) and execution.exists():
        result['action_cpu_seconds'] = sum(cpu_by_mnemonic.values())
        result['action_cpu_reason'] = 'BEP actionSummary.actionData userTime+systemTime for every executed spawn mnemonic; not JVM/client CPU'
    return result


def buck_telemetry(path):
    """Buck2 2026-10-15 data.proto ActionExecutionKind, not console heuristics.

    Schema: https://github.com/facebook/buck2/blob/2026-10-15/app/buck2_data/data.proto
    Successful cargo_build/cargo_test actions each invoke one rustc. Failed
    actions may stop during input population, before any compiler process.
    Native subprocesses inside run-custom-build are not inferred.
    """
    kinds = {0: 'unset', 1: 'local', 2: 'remote', 3: 'action_cache', 4: 'simple',
             6: 'deferred', 7: 'local_dep_file', 8: 'local_worker',
             9: 'remote_dep_file_cache', 10: 'local_action_cache',
             11: 'remote_worker', 12: 'remote_served_from_cache', 13: 'remote_deduplicated'}
    result = {'compiler_executions': 0, 'compiler_action_attempts': 0, 'failed_compiler_actions': 0,
              'execution_kinds': {}, 'executed_categories': {}, 'action_keys': [],
              'failed_actions': 0, 'native_compiler_subprocesses': None,
              'compiler_count_definition': 'one rustc per successful cargo_build/cargo_test action; total unmeasured if an eligible action fails; buildscript native subprocesses unmeasured',
              'native_event_json': str(path), 'snapshots': []}
    memory_peaks = []
    memory_unmeasured = 0
    trace_ids = set()
    commands_complete = False
    for row in json_stream(path):
        # CommandEnd.is_success is deprecated and defaults false even on success.
        # CommandResult is authoritative for these native build/test operations.
        response = row.get('Result', {}).get('result', {})
        if 'build_response' in response:
            result['command_success'] = not response['build_response'].get('errors')
        elif 'test_response' in response:
            test = response['test_response']
            result['command_success'] = (test.get('executor_exit_code') == 0
                                         and not test.get('build_errors') and not test.get('test_errors'))
        elif 'error' in response:
            result['command_success'] = False
        trace_id = row.get('Event', {}).get('trace_id')
        if trace_id:
            trace_ids.add(trace_id)
        event = row.get('Event', {}).get('data', {})
        end = event.get('SpanEnd', {}).get('data', {})
        if 'Command' in end:
            commands_complete = True
        snapshot = event.get('Instant', {}).get('data', {}).get('Snapshot')
        if snapshot:
            result['snapshots'].append({key: value for key, value in snapshot.items()
                                        if not isinstance(value, (dict, list))})
        action = end.get('ActionExecution')
        if not action:
            continue
        kind = action.get('execution_kind', 0)
        name = kinds.get(kind, 'unknown-' + str(kind))
        result['execution_kinds'][name] = result['execution_kinds'].get(name, 0) + 1
        category = action.get('name', {}).get('category', 'unknown')
        if kind in (1, 2, 8, 11):
            result['executed_categories'][category] = result['executed_categories'].get(category, 0) + 1
            if category in ('cargo_build', 'cargo_test'):
                result['compiler_executions'] += 1
        if category in ('cargo_build', 'cargo_test') and (kind in (1, 2, 8, 11) or action.get('failed')):
            result['compiler_action_attempts'] += 1
        if category in ('cargo_build', 'cargo_test') and action.get('failed'):
            result['failed_compiler_actions'] += 1
        if action.get('failed'):
            result['failed_actions'] += 1
        digests = []
        for command in action.get('commands', []):
            variants = command.get('details', {}).get('command_kind', {}).get('command', {})
            stats = command.get('details', {}).get('metadata', {}).get('execution_stats') or {}
            if kind in (1, 2, 8, 11):
                if stats.get('memory_peak') is not None and int(stats['memory_peak']) > 0:
                    memory_peaks.append(int(stats['memory_peak']))
                else:
                    memory_unmeasured += 1
            for detail in variants.values():
                if isinstance(detail, dict) and detail.get('action_digest'):
                    digests.append(detail['action_digest'])
        result['action_keys'].append({'key': action.get('key'), 'category': category,
                                      'failed': bool(action.get('failed')),
                                      'execution_kind': name, 'action_digests': digests,
                                      'commands': action.get('commands', []),
                                      'cache_upload_result': action.get('cache_upload_result')})
    result['command_end_observed'] = commands_complete
    result['trace_ids'] = sorted(trace_ids)
    result['successful_compiler_actions'] = result['compiler_executions'] - sum(
        action.get('category') in ('cargo_build', 'cargo_test') and action.get('failed', False)
        and action['execution_kind'] in ('local', 'remote', 'local_worker', 'remote_worker')
        for action in result['action_keys'])
    if not commands_complete or result['failed_compiler_actions']:
        result['compiler_executions'] = None
    # Endpoints suffice for daemon metrics; all original samples remain in JSON.
    result['snapshots'] = result['snapshots'][:1] + result['snapshots'][-1:]
    result['maximum_executed_action_memory_bytes'] = max(memory_peaks) if memory_peaks else None
    result['memory_sample_count'] = len(memory_peaks)
    result['memory_unmeasured_command_count'] = memory_unmeasured
    result['memory_method'] = 'largest positive native execution_stats.memory_peak, not concurrent RSS; zero/missing defaults unmeasured'
    result['action_cpu_seconds'] = None
    result['action_cpu_reason'] = 'not inferred from client CPU or summed wall time; per-command native worker details retained when reported'
    return result


def artifact_markers(data, directory, decoder, command):
    """Inspect native bytes and real LLVM archive members, never source text."""
    pattern = rb'sekka-bench-(?:(?!sekka-bench-)[a-zA-Z0-9-])+?-END'
    found = set(re.findall(pattern, data))
    members = []
    if data.startswith(b'!<arch>\n'):
        offset = 8
        while offset < len(data):
            header = data[offset:offset + 60]
            if len(header) != 60 or header[58:] != b'`\n':
                raise RuntimeError('malformed rlib archive member header')
            size = int(header[48:58])
            start = offset + 60
            end = start + size
            if size < 0 or end > len(data):
                raise RuntimeError('truncated rlib archive member')
            name = header[:16].rstrip().decode('ascii')
            payload = data[start:end]
            if name.startswith('#1/'):
                name_size = int(name[3:])
                if name_size > len(payload):
                    raise RuntimeError('truncated BSD archive member name')
                name = payload[:name_size].rstrip(b'\0').decode('utf-8')
                payload = payload[name_size:]
            if payload.startswith((b'BC\xc0\xde', b'\xde\xc0\x17\x0b')):
                if not decoder:
                    raise RuntimeError('bitcode-only rlib requires matching bitcode_decoder argv')
                prefix = Path(directory) / ('bitcode-' + str(len(members)))
                source = prefix.with_suffix('.bc')
                decoded = prefix.with_suffix('.ll')
                source.parent.mkdir(parents=True, exist_ok=True)
                with source.open('xb') as stream:
                    stream.write(payload)
                rec = command(expand(decoder, {'member': source, 'decoded': decoded}), prefix.name)
                if rec['exit_status'] or not decoded.is_file():
                    raise RuntimeError(f'actual rlib bitcode decoding failed: {source}')
                ir = decoded.read_bytes()
                found.update(re.findall(pattern, ir))
                members.append({'archive_name': name, 'archive_offset': start,
                                'bitcode': str(source), 'sha256': digest(payload),
                                'decoded': str(decoded), 'decoded_sha256': digest(ir), 'command': rec})
            offset = end + size % 2
    return {'markers': sorted(value.decode() for value in found), 'bitcode_members': members}


def marker_check(found, required, current):
    observed = set(found)
    expected = set(required)
    allowed = {marker for marker in current if marker}
    return {'markers': sorted(observed), 'expected': sorted(expected),
            'allowed_current_markers': sorted(allowed),
            'missing_required_markers': sorted(expected - observed),
            'unexpected_markers': sorted(observed - allowed),
            'passed': expected <= observed <= allowed}


def cache_inventory_policy(inventory):
    """Check retained-cache policy accounting, not physical or total-state usage."""
    roots = inventory.get('roots')
    if (type(inventory.get('schema')) is not int or inventory['schema'] != 1
            or inventory.get('errors') != [] or not isinstance(roots, list) or not roots
            or not all(isinstance(root, str) and root for root in roots)
            or len(set(roots)) != len(roots)):
        raise RuntimeError('invalid or incomplete owned-cache inventory')
    fields = ('root_apparent_bytes', 'root_block_accounted_bytes', 'root_entry_counts',
              'configured_root_byte_limits', 'configured_root_entry_limits')
    for field in fields:
        values = inventory.get(field)
        if (not isinstance(values, list) or len(values) != len(roots)
                or any(type(value) is not int or value < 0 for value in values)):
            raise RuntimeError(f'invalid owned-cache inventory column: {field}')
    rows = []
    for index, root in enumerate(roots):
        row = {'root': root, **{field: inventory[field][index] for field in fields}}
        if (row['root_block_accounted_bytes'] < row['root_apparent_bytes']
                or row['configured_root_byte_limits'] == 0
                or row['configured_root_entry_limits'] == 0):
            raise RuntimeError('inconsistent owned-cache policy accounting')
        row['within_byte_limit'] = row['root_block_accounted_bytes'] <= row['configured_root_byte_limits']
        row['within_entry_limit'] = row['root_entry_counts'] <= row['configured_root_entry_limits']
        rows.append(row)
    return {'roots': rows,
            'passed': all(row['within_byte_limit'] and row['within_entry_limit'] for row in rows),
            'note': 'retained-cache snapshot policy bounds; NOT physical bytes or total-state enforcement'}


class Bench:
    def __init__(self, config):
        self.config = config
        self.driver_sha256 = file_digest(__file__)
        self.root = Path(config['root']).resolve()
        self.repo = Path(config['repo']).resolve()
        self.commit = config['commit']
        if not re.fullmatch('[0-9a-f]{40}', self.commit):
            raise ValueError('commit must be the frozen full 40-character SHA')
        if config['tool'] not in ('bazel', 'buck2'):
            raise ValueError('tool must be bazel or buck2')
        self.env = dict(os.environ, **config.get('environment', {}))
        self.plan = plan(config.get('plan', 'full'))
        self.fingerprint = digest(canonical(config))
        self.root.mkdir(parents=True, exist_ok=True, mode=0o700)
        marker = self.root / OWNER
        if marker.exists():
            self.owner = load(marker)
            if self.owner['config_sha256'] != self.fingerprint:
                raise ValueError('resume configuration changed; use a NEW owned run root')
        else:
            if any(self.root.iterdir()):
                raise ValueError('new run root must be empty')
            self.owner = {'schema': SCHEMA, 'run_id': str(uuid.uuid4()), 'config_sha256': self.fingerprint,
                          'source_commit': self.commit, 'created': time.time(), 'host': socket.gethostname()}
            save(marker, self.owner)
            save(self.root / 'config.json', config)
            save(self.root / 'plan.json', self.plan)
        self.lock = (self.root / '.lock').open('a')
        fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.original = {}
        for relative in (LEAF, SHARED):
            self.original[relative] = subprocess.check_output(['git', '-C', str(self.repo), 'show', f'{self.commit}:{relative}'])

    def values(self, tree='a', profile='dev', jobs=16, trial='setup'):
        return {'tree': self.root / 'trees' / tree, 'root': self.root, 'profile': profile,
                'jobs': jobs, 'trial': self.root / 'trials' / trial, 'state': self.root / 'state' / tree,
                'output': self.root / 'state' / tree / 'outputs', 'python': sys.executable}

    def command(self, argv, cwd, directory, name, *, timeout=None, env=None, start_barrier=None, separate_stderr=False):
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=True)
        log = directory / (name + '.log')
        record = {'argv': argv, 'cwd': str(cwd), 'intent_unix': time.time(),
                  'load_before': list(os.getloadavg()), 'log': str(log)}
        if separate_stderr:
            record['stderr_log'] = str(directory / (name + '.stderr.log'))
        # The intent is durable before spawning, so interruption remains visible.
        save(directory / (name + '.intent.json'), record)
        # Evidence publication/fsync is not charged to the build.
        if start_barrier is not None:
            start_barrier.wait(timeout=self.config.get('barrier_timeout_seconds', 300))
        mono = time.monotonic()
        record.update(start_unix=time.time(), start_monotonic=mono)
        try:
            with contextlib.ExitStack() as stack:
                stream = stack.enter_context(log.open('xb'))
                stderr = (stack.enter_context(Path(record['stderr_log']).open('xb'))
                          if separate_stderr else subprocess.STDOUT)
                proc = subprocess.Popen(argv, cwd=cwd, env=env or self.env, stdout=stream,
                                        stderr=stderr, start_new_session=True)
                try:
                    status = proc.wait(timeout=timeout or self.config.get('timeout_seconds', 7200))
                except subprocess.TimeoutExpired:
                    import signal
                    os.killpg(proc.pid, signal.SIGTERM)
                    try:
                        proc.wait(timeout=20)
                    except subprocess.TimeoutExpired:
                        os.killpg(proc.pid, signal.SIGKILL)
                        proc.wait()
                    status = 124
                    record['timed_out'] = True
        except OSError as exc:
            status = 127
            record['launch_error'] = str(exc)
        ended = time.monotonic()
        record.update(exit_status=status, wall_seconds=ended - mono, end_monotonic=ended,
                      end_unix=time.time(), load_after=list(os.getloadavg()))
        save(directory / (name + '.json'), record)
        return record

    def tree(self, name):
        path = self.root / 'trees' / name
        if path.is_symlink() or not path.resolve().is_relative_to(self.root):
            raise RuntimeError(f'refusing unowned/symlink worktree: {path}')
        if path.exists():
            head = subprocess.check_output(['git', '-C', str(path), 'rev-parse', 'HEAD'], text=True).strip()
            if head != self.commit:
                raise RuntimeError(f'owned tree changed HEAD: {path}')
            return path
        path.parent.mkdir(exist_ok=True)
        directory = self.root / 'setup' / ('tree-' + name)
        attempt = directory / str(time.time_ns())
        result = self.command(['git', 'clone', '--shared', '--no-checkout', str(self.repo), str(path)], self.repo, attempt, 'clone')
        if result['exit_status']:
            raise RuntimeError(f'clone failed: {attempt}')
        result = self.command(['git', '-C', str(path), 'checkout', '--detach', self.commit], self.repo, attempt, 'checkout')
        if result['exit_status']:
            raise RuntimeError(f'checkout failed: {attempt}')
        return path

    def client(self, values, operation, extra=(), frontend_extra=()):
        config = self.config
        base = expand(config['client'], values)
        profile = str(values['profile'])
        if config['tool'] == 'bazel':
            base += ['--profile', profile, '--output-base', str(values['output']),
                     '--disk-cache', str(self.root / 'cache' / 'bazel')]
            base += expand(config.get('client_flags', []), values)
            return base + list(frontend_extra) + [operation] + list(extra)
        # Isolation participates in Buck action paths: keep it identical across
        # fresh trees, machines, and output-loss recovery, not random per run.
        isolation = config.get('buck_isolation', 'shared-bench')
        base += ['--profile', profile, '--isolation-dir', isolation, '-j', str(values['jobs'])]
        base += expand(config.get('client_flags', []), values)
        return base + list(frontend_extra) + [operation] + list(extra)

    def prepare(self):
        complete = self.root / 'setup' / 'complete.json'
        if complete.exists():
            return
        attempt = self.root / 'setup' / ('attempt-' + str(time.time_ns()))
        tree = self.tree('a')
        values = self.values()
        records = []
        for index, argv in enumerate(self.config.get('setup', [])):
            rec = self.command(expand(argv, values), tree, attempt, f'{index:03d}')
            records.append(rec)
            if rec['exit_status']:
                raise RuntimeError(f'setup failed: {rec["log"]}')
        source = {'commit': self.commit, 'repo': str(self.repo), 'hostname': socket.gethostname(),
                  'cpu_count': os.cpu_count(), 'platform': list(os.uname()), 'environment_overrides': self.config.get('environment', {}),
                  'nix_identity': self.config.get('nix_identity', []), 'variants': self.config.get('variants', []),
                  'service_identity': self.config.get('service', {}).get('identity'),
                  'setup_records': records, 'excluded_from_build_timings': True}
        source['driver_sha256'] = self.driver_sha256
        source['nix_identity_files'] = [
            {'path': str(path), 'sha256': file_digest(path)}
            for path in self.config.get('nix_identity', [])]
        if self.config.get('cargo_baseline'):
            source['cargo_baseline'] = {'path': self.config['cargo_baseline'],
                                        'sha256': file_digest(self.config['cargo_baseline'])}
        source['comparison_identity'] = self.config.get('comparison_identity')
        source['locks'] = {rel: file_digest(tree / rel) for rel in self.config.get('source_locks', ['flake.lock', 'desktop/aizome/Cargo.lock'])}
        save(complete, source)

    def edit_source(self, name, stage):
        tree = self.tree(name)
        desired = {key: value.replace('{tree}', name) for key, value in stage['edits'].items()}
        markers = {key: 'sekka-bench-' + token + '-END' for key, token in desired.items()}
        old_state = tree / '.sekka-bench-source.json'
        state = load(old_state) if old_state.exists() else {'files': {}}
        updates = dict(self.original)
        if 'leaf' in markers:
            anchor = b'fn main() {'
            if updates[LEAF].count(anchor) != 1:
                raise RuntimeError('leaf function anchor is not unique')
            injection = ('\n    if std::env::args().any(|arg| arg == "--sekka-bench-probe") {\n'
                         f'        println!("{markers["leaf"]}");\n        return;\n    }}').encode()
            updates[LEAF] = updates[LEAF].replace(anchor, anchor + injection, 1)
        if 'shared' in markers:
            # A public callable API survives in the rlib; no linker-collectable #[used] static.
            updates[SHARED] += ('\n#[inline(never)]\npub fn sekka_bench_probe() -> &\'static str {\n'
                                f'    "{markers["shared"]}"\n}}\n').encode()
        icon_paths = set(state.get('icons', []))
        new_icons = []
        if 'icon' in markers:
            rel = ICONS + '/' + markers['icon'] + '.svg'
            updates[rel] = b'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" /></svg>\n'
            new_icons.append(rel)
        changes = []
        for rel in sorted(set(updates) | icon_paths):
            path = tree / rel
            if path.is_symlink() or not path.resolve().is_relative_to(tree.resolve()):
                raise RuntimeError(f'refusing source path outside owned worktree: {path}')
            before = path.read_bytes() if path.exists() else None
            expected = state['files'].get(rel)
            if expected is not None and (before is None or digest(before) != expected):
                raise RuntimeError(f'refusing to overwrite unexpected source change: {path}')
            if expected is None and rel in self.original and before != self.original[rel]:
                raise RuntimeError(f'initial tracked source differs: {path}')
            after = updates.get(rel)
            if before == after:
                continue  # Preserve mtimes on noops and unchanged paths.
            if after is None:
                path.unlink()
            else:
                if expected is None and rel not in self.original and before is not None:
                    raise RuntimeError(f'icon path already exists: {path}')
                path.write_bytes(after)
            changes.append({'path': rel, 'before': digest(before) if before is not None else None,
                            'after': digest(after) if after is not None else None})
        new_state = {'files': {rel: digest(data) for rel, data in updates.items()}, 'icons': new_icons}
        if new_state != state or not old_state.exists():
            temporary = old_state.with_suffix('.tmp')
            temporary.write_bytes(canonical(new_state))
            temporary.replace(old_state)
        return markers, changes

    def telemetry_flags(self, prefix, values):
        flags = expand(self.config['buck_telemetry_flags'], dict(values, telemetry=prefix))
        if any(flag.startswith('--buck-arg=--write-build-id') for flag in flags):
            raise ValueError('write-build-id is owned by the harness, not buck_telemetry_flags')
        return flags + [f'--buck-arg=--write-build-id={prefix}.build-id']

    def collect_telemetry(self, prefix, values, directory, name='telemetry', invocation=None):
        if self.config['tool'] == 'bazel':
            return bazel_telemetry(prefix)
        event_path = Path(str(prefix) + '.events.json-lines.gz')
        build_id_path = Path(str(prefix) + '.build-id')
        final_id = build_id_path.read_text().strip() if build_id_path.exists() else None
        uuid_pattern = r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}'
        if final_id is not None and not re.fullmatch(uuid_pattern, final_id):
            raise RuntimeError('invalid native write-build-id identity')
        attempt_ids = []
        for key in ('log', 'stderr_log'):
            if invocation and invocation.get(key):
                for ident in re.findall(r'Build ID: (' + uuid_pattern + r')',
                                        Path(invocation[key]).read_text(errors='replace')):
                    if ident not in attempt_ids:
                        attempt_ids.append(ident)
        logged_attempt_ids = list(attempt_ids)
        if final_id and final_id not in attempt_ids:
            attempt_ids.append(final_id)
        engine = self.config.get('buck_engine')
        if not engine and self.env.get('SEKKA_BUCK2_BIN'):
            engine = [self.env['SEKKA_BUCK2_BIN']]
        if not engine:
            raise RuntimeError('buck_engine argv required to decode native events')
        engine = expand(engine, values) + [
            '--isolation-dir', self.config.get('buck_isolation', 'shared-bench')]
        attempts = []
        errors = []
        # Preserve the explicitly requested file even when native retry left it
        # pointing at the FIRST attempt. Never ask Buck for an implicit latest log.
        if event_path.exists():
            decoded = self.command(engine + ['log', 'show', str(event_path)],
                                   values['tree'], directory, name + '-requested', separate_stderr=True)
            if decoded['exit_status'] == 0:
                telemetry = buck_telemetry(decoded['log'])
                telemetry['collection'] = decoded
                if len(telemetry['trace_ids']) == 1:
                    ident = telemetry['trace_ids'][0]
                    attempts.append(dict(trace_id=ident, telemetry=telemetry))
                    if ident not in attempt_ids:
                        attempt_ids.insert(0, ident)
                else:
                    errors.append('requested event file does not identify exactly one trace')
            else:
                errors.append('requested native event file could not be decoded')
        for ident in attempt_ids:
            if any(attempt['trace_id'] == ident for attempt in attempts):
                continue
            decoded = self.command(engine + ['log', 'show', '--trace-id', ident, '--no-remote'],
                                   values['tree'], directory, name + '-' + ident, separate_stderr=True)
            if decoded['exit_status']:
                attempts.append({'trace_id': ident, 'collection': decoded,
                                 'error': 'exact attempt event log unavailable; execution remains unmeasured'})
                continue
            telemetry = buck_telemetry(decoded['log'])
            telemetry['collection'] = decoded
            if telemetry['trace_ids'] != [ident]:
                attempts.append({'trace_id': ident, 'collection': decoded,
                                 'error': 'decoded event identity differs from requested trace'})
                continue
            attempts.append(dict(trace_id=ident, telemetry=telemetry))
        # Decode what-ran from our already retained exact JSON, not the rotating
        # daemon log directory. Capture ALL events before any additional commands.
        for attempt in attempts:
            telemetry = attempt.get('telemetry')
            if telemetry is None:
                continue
            retained = Path(directory) / (name + '-' + attempt['trace_id'] + '.json-lines')
            os.link(telemetry['native_event_json'], retained)
            telemetry['retained_native_event_json'] = str(retained)
            executed = self.command(
                engine + ['log', 'what-ran', '--format', 'json', '--skip-cache-hits',
                          str(retained)],
                values['tree'], directory, name + '-' + attempt['trace_id'] + '-executed', separate_stderr=True)
            telemetry['execution_records'] = executed
            if executed['exit_status']:
                attempt['error'] = 'native exact-attempt execution-record decoding failed'
                continue
            records = list(json_stream(executed['log']))
            telemetry['execution_reasons'] = {}
            for row in records:
                reason = row.get('reason', 'unknown')
                telemetry['execution_reasons'][reason] = telemetry['execution_reasons'].get(reason, 0) + 1
            telemetry['test_executions'] = [row for row in records if row.get('reason') == 'test.run']
            telemetry['test_execution_count'] = len(telemetry['test_executions'])
        by_id = {attempt['trace_id']: attempt for attempt in attempts}
        attempts = [by_id[ident] for ident in attempt_ids if ident in by_id]
        final = by_id.get(final_id, {}).get('telemetry', {})
        complete = bool(final_id and logged_attempt_ids and logged_attempt_ids[-1] == final_id
                        and attempts and final) and not errors and all(
            'error' not in attempt and attempt.get('telemetry', {}).get('command_end_observed')
            and attempt['telemetry'].get('command_success') is not None
            for attempt in attempts)
        result = dict(final)
        result.update(attempts=attempts, final_trace_id=final_id, build_id_path=str(build_id_path),
                      collection_errors=errors, complete_attempt_collection=complete,
                      final_compiler_executions=final.get('compiler_executions'),
                      compiler_count_definition='total across every retained attempt; unknown if any attempt is incomplete or has failed compiler actions; final attempt reported separately')
        result['logged_attempt_ids'] = logged_attempt_ids
        counts = [attempt.get('telemetry', {}).get('compiler_executions') for attempt in attempts]
        result['compiler_executions'] = sum(counts) if complete and None not in counts else None
        result['failed_native_attempts'] = sum(
            attempt.get('telemetry', {}).get('command_success') is False for attempt in attempts)
        peaks = [attempt.get('telemetry', {}).get('maximum_executed_action_memory_bytes') for attempt in attempts]
        result['maximum_executed_action_memory_bytes'] = max((peak for peak in peaks if peak is not None), default=None)
        for key in ('memory_sample_count', 'memory_unmeasured_command_count'):
            result[key] = sum(attempt.get('telemetry', {}).get(key, 0) for attempt in attempts)
        for key in ('compiler_action_attempts', 'successful_compiler_actions', 'failed_compiler_actions', 'failed_actions'):
            result[key] = sum(attempt.get('telemetry', {}).get(key, 0) for attempt in attempts)
        result['observed_work_counts_complete'] = complete
        result['execution_kinds'] = {}
        result['executed_categories'] = {}
        result['action_keys'] = []
        for attempt in attempts:
            telemetry = attempt.get('telemetry', {})
            for kind, count in telemetry.get('execution_kinds', {}).items():
                result['execution_kinds'][kind] = result['execution_kinds'].get(kind, 0) + count
            for category, count in telemetry.get('executed_categories', {}).items():
                result['executed_categories'][category] = result['executed_categories'].get(category, 0) + count
            result['action_keys'].extend(dict(action, trace_id=attempt['trace_id'])
                                         for action in telemetry.get('action_keys', []))
        save(Path(directory) / (name + '-attempts.json'), result)
        return result

    def artifact(self, kind, values, directory, require_cached=True):
        selector = self.config['artifacts'][kind]
        selector = selector.get(str(values['profile']), selector)
        target = selector['target']
        prefix = directory / (kind + '-materialize-native')
        if self.config['tool'] == 'bazel':
            groups = ['--output_groups=' + selector['output_group']] if selector.get('output_group') else []
            # Only the inspected declared output is forced local, never its
            # deliberately lazy dependency closure.
            flags = [f'--build_event_json_file={prefix}.bep.json', f'--execution_log_json_file={prefix}.exec.json']
            materialize = self.client(values, 'build', ['--remote_download_outputs=toplevel', *flags, *groups, target])
            rec = self.command(materialize, values['tree'], directory, kind + '-materialize')
            if rec['exit_status']:
                raise RuntimeError(f'artifact materialization failed: {kind}')
            argv = self.client(values, 'cquery', ['--output=files', *groups, target])
            query = self.command(argv, values['tree'], directory, kind + '-declared', separate_stderr=True)
            if query['exit_status']:
                raise RuntimeError(f'artifact declaration query failed: {kind}')
            lines = Path(query['log']).read_text().splitlines()
            candidates = [Path(values['tree']) / line for line in lines if line.startswith(('bazel-out/', '/'))]
        else:
            flags = self.telemetry_flags(prefix, values)
            argv = self.client(values, 'outputs', [*flags, target])
            rec = self.command(argv, values['tree'], directory, kind + '-declared-materialize')
            if rec['exit_status']:
                raise RuntimeError(f'artifact declaration/materialization failed: {kind}')
            text = Path(rec['log']).read_text()
            # Buck --show-full-json-output maps declared labels to full output paths.
            decoder = json.JSONDecoder()
            objects = []
            for match in re.finditer(r'(?m)^\{', text):
                try:
                    obj, _ = decoder.raw_decode(text[match.start():])
                    if isinstance(obj, dict):
                        objects.append(obj)
                except ValueError:
                    pass
            paths = []
            for obj in objects:
                for value in obj.values():
                    if isinstance(value, str):
                        paths.append(value)
                    elif isinstance(value, list):
                        paths.extend(x for x in value if isinstance(x, str))
            candidates = [Path(values['tree']) / value for value in paths]
        telemetry = self.collect_telemetry(prefix, values, directory, kind + '-materialize-telemetry', invocation=rec)
        work = sum(telemetry.get('executed_mnemonics', telemetry.get('executed_categories', {})).values())
        save(directory / (kind + '-materialization.json'), {
            'telemetry': telemetry, 'executed_work_actions': work, 'require_cached': require_cached,
            'note': 'verification cannot silently rebuild missing CAS outputs outside the measured build'})
        if telemetry.get('compiler_executions') is None or (require_cached and work):
            raise RuntimeError(f'{kind}: artifact verification executed {work} work actions or lacks native telemetry')
        pattern = selector.get('file_regex')
        if pattern:
            candidates = [path for path in candidates if re.search(pattern, str(path))]
        if selector.get('member'):
            candidates = [path / selector['member'] for path in candidates]
        candidates = list(dict.fromkeys(candidates))
        if len(candidates) != 1:
            raise RuntimeError(f'{kind}: expected one declared artifact, got {candidates}')
        path = candidates[0]
        if not path.is_file():
            raise RuntimeError(f'declared artifact absent after materialization: {path}')
        return path

    def check(self, markers, values, directory, reference=None, kinds=('leaf', 'shared', 'icon')):
        checks = {}
        for kind in kinds:
            path = self.artifact(kind, values, directory)
            data = path.read_bytes()
            marker = markers.get(kind)
            inspection = artifact_markers(
                data, directory / (kind + '-inspection'),
                expand(self.config.get('bitcode_decoder', []), values),
                lambda argv, name: self.command(argv, values['tree'], directory / (kind + '-inspection'), name))
            found = inspection['markers']
            expected = [marker] if marker else []
            check = {'artifact': str(path), 'sha256': digest(data), 'bytes': len(data),
                     **marker_check(found, expected, markers.values())}
            check['bitcode_members'] = inspection['bitcode_members']
            if kind == 'leaf' and marker:
                selector = self.config['artifacts'][kind]
                selector = selector.get(str(values['profile']), selector)
                probe_argv = expand(selector.get('probe', ['{artifact}', '--sekka-bench-probe']),
                                    dict(values, artifact=path))
                probe = self.command(probe_argv, values['tree'], directory, 'leaf-behavior')
                stdout = Path(probe['log']).read_text().strip()
                check['behavior'] = {'exit_status': probe['exit_status'], 'stdout': stdout}
                check['passed'] &= probe['exit_status'] == 0 and stdout == marker
            if reference and kind in reference:
                check['reference_sha256'] = reference[kind]['sha256']
                # Reproducibility is measured separately from semantic correctness.
                check['reference_bytes_equal'] = check['sha256'] == reference[kind]['sha256']
            checks[kind] = check
        if 'leaf' in kinds and not markers.get('leaf') and self.config.get('correctness_behavior'):
            behavior = expand(self.config['correctness_behavior'], values)
            behavior_prefix = directory / 'original-behavior-native'
            if self.config['tool'] == 'buck2':
                behavior += self.telemetry_flags(behavior_prefix, values)
            rec = self.command(behavior, values['tree'], directory, 'original-behavior')
            counts = self.test_counts(rec['log'])
            checks['original_behavior'] = {
                'command': rec, 'test_counts': counts,
                'passed': rec['exit_status'] == 0 and counts['passed'] == 11 and counts['failed'] == 0}
            if self.config['tool'] == 'buck2':
                telemetry = self.collect_telemetry(behavior_prefix, values, directory, 'original-behavior-telemetry', invocation=rec)
                checks['original_behavior']['telemetry'] = telemetry
                checks['original_behavior']['passed'] &= (telemetry.get('test_execution_count', 0) > 0
                    and telemetry.get('complete_attempt_collection', False) and telemetry.get('command_success') is True)
        return checks

    def metrics(self, values, directory, name):
        argv = self.config.get('service', {}).get('metrics')
        if not argv:
            return {'available': False}
        return self.command(expand(argv, values), values['tree'], directory, name)

    def lose_outputs(self, values, directory):
        if self.config['tool'] == 'bazel':
            argv = self.client(values, 'shutdown')
            output = Path(values['output'])
        else:
            isolation = self.config.get('buck_isolation', 'shared-bench')
            argv = expand(self.config['buck_engine'], values) + ['--isolation-dir', isolation, 'kill']
            output = Path(values['tree']) / 'buck-out' / isolation
        output = output.resolve()
        if not output.is_relative_to(self.root) or not output.is_dir():
            raise RuntimeError(f'output loss requires an existing owned output root: {output}')
        stopped = self.command(argv, values['tree'], directory, 'stop-before-output-loss')
        if stopped['exit_status']:
            raise RuntimeError('refusing output loss while owned build daemon may still be running')
        retained = self.root / 'retained-output-loss' / directory.parent.name / directory.name
        retained.parent.mkdir(parents=True, exist_ok=True)
        if retained.exists():
            raise RuntimeError(f'output loss evidence already exists: {retained}')
        output.rename(retained)
        save(directory / 'output-loss.json', {
            'old_output_root': str(output), 'retained_evidence': str(retained),
            'same_action_path_on_recovery': True, 'cache_deleted': False,
            'note': 'mutable state moved out of active roots, not destroyed; exclude retained evidence from active union'})

    def trial(self, stage, name, directory, prepared=None, start_barrier=None):
        values = self.values(name, stage['profile'], stage['jobs'], stage['id'] + '-' + name)
        tree = self.tree(name)
        markers, changes = prepared if prepared is not None else self.edit_source(name, stage)
        if stage.get('output_loss'):
            self.lose_outputs(values, directory)
        profile = self.config['profiles'][stage['profile']]
        targets = stage.get('targets', profile['targets'])
        operation = stage['operation']
        extra = list(targets)
        if operation == 'test':
            extra = self.config['test_groups'][stage['test_group']]['args']
        elif operation == 'soroban':
            extra = self.config['soroban_tests']['args']
        prefix = directory / 'native'
        build_info = None
        if operation == 'startup':
            if self.config['tool'] == 'buck2':
                runtime_dir = directory / 'runtime'
                runtime = self.command(
                    self.client(values, 'runtime', frontend_extra=['--runtime-dir', str(runtime_dir)]),
                    tree, directory, 'runtime-discovery')
                if runtime['exit_status']:
                    raise RuntimeError('ergonomic Buck runtime materialization failed')
                manifest = load(runtime_dir / 'runtime.json')
                for runtime_name, target_name in self.config['runtime_targets'].items():
                    path = Path(manifest['binaries'][target_name])
                    if not path.is_file():
                        raise RuntimeError(f'declared runtime launcher missing: {path}')
                    values[runtime_name] = path
            else:
                for runtime_name, selector in self.config['runtime_artifacts'].items():
                    values[runtime_name] = self.artifact(selector, values, directory, require_cached=False)
            if 'compositor' in values:
                build_info = self.command([str(values['compositor']), '--build-info'], tree, directory, 'build-info')
            argv = expand(self.config['startup'], values)
            startup_result = Path(expand([self.config['startup_result']], values)[0])
            if startup_result.exists():
                raise RuntimeError(f'refusing stale private startup evidence: {startup_result}')
        else:
            native_operation = 'test' if operation in ('test', 'soroban') else 'build'
            if self.config['tool'] == 'bazel':
                telemetry_flags = [f'--jobs={stage["jobs"]}', f'--build_event_json_file={prefix}.bep.json',
                                   f'--execution_log_json_file={prefix}.exec.json', f'--profile={prefix}.profile.gz']
                if native_operation == 'test':
                    telemetry_flags += ['--nocache_test_results', '--test_output=all']
            else:
                telemetry_flags = self.telemetry_flags(prefix, values)
            argv = self.client(values, native_operation, telemetry_flags + extra)
        patch = subprocess.check_output(['git', '-C', str(tree), 'diff', '--binary', 'HEAD'])
        directory.mkdir(parents=True, exist_ok=True)
        (directory / 'source.patch').write_bytes(patch)
        source = {'commit': self.commit, 'tracked_patch_sha256': digest(patch), 'changes': changes,
                  'owned_files': load(tree / '.sekka-bench-source.json'), 'markers': markers}
        untracked = subprocess.check_output(
            ['git', '-C', str(tree), 'ls-files', '--others', '--exclude-standard', '-z']).split(b'\0')
        source['untracked_files'] = {
            os.fsdecode(relative): file_digest(tree / os.fsdecode(relative))
            for relative in untracked if relative and (tree / os.fsdecode(relative)).is_file()
        }
        source['identity_sha256'] = digest(canonical({
            'commit': self.commit, 'patch': source['tracked_patch_sha256'],
            'untracked': source['untracked_files']}))
        before = self.metrics(values, directory, 'metrics-before')
        rec = self.command(argv, tree, directory, 'build', start_barrier=start_barrier)
        if start_barrier is not None:
            # A fast worker must not compile correctness tests or materialize
            # artifacts while another worker is still in its measured build.
            start_barrier.wait(timeout=self.config.get('timeout_seconds', 7200) + 30)
        result = {'schema': SCHEMA, 'stage': stage, 'tree': name, 'tool': self.config['tool'],
                  'profile': stage['profile'], 'features': profile['features'], 'source': source,
                  'host': socket.gethostname(), 'service': self.config.get('service', {}).get('identity'),
                  'variants': self.config.get('variants', []), 'build': rec,
                  'metrics_before': before, 'metrics_after': self.metrics(values, directory, 'metrics-after')}
        result['driver_sha256'] = self.driver_sha256
        if build_info is not None:
            result['build_info'] = build_info
        try:
            result['telemetry'] = (self.collect_telemetry(prefix, values, directory, invocation=rec) if operation != 'startup'
                                   else {'compiler_executions': None, 'not_applicable': 'runtime execution'})
            if operation != 'startup' and rec['exit_status'] == 0:
                telemetry = result['telemetry']
                if self.config['tool'] == 'buck2':
                    if not telemetry.get('complete_attempt_collection') or telemetry.get('command_success') is not True:
                        raise RuntimeError('successful build lacks complete exact-attempt native telemetry and successful final trace')
                elif telemetry.get('compiler_executions') is None:
                    raise RuntimeError('successful build lacks complete native compiler-action telemetry')
        except Exception as exc:
            result['telemetry_error'] = str(exc)
        try:
            if rec['exit_status'] == 0 and operation == 'build':
                reference = self.reference(stage.get('reference'))
                result['correctness'] = self.check(markers, values, directory, reference,
                                                   stage.get('checks', ('leaf', 'shared', 'icon')))
            elif rec['exit_status'] == 0 and operation == 'startup':
                report = load(startup_result)
                executables_match = all(
                    report.get('executables', {}).get(runtime_name) == str(Path(values[runtime_name]).resolve())
                    for runtime_name in ('compositor', 'shell', 'ctl', 'koe'))
                screenshots = {}
                for filename in ('koe-window-live.png', 'startup-success-live.png'):
                    image = startup_result.parent / filename
                    with image.open('rb') as stream:
                        screenshots[filename] = stream.read(8) == b'\x89PNG\r\n\x1a\n' and image.stat().st_size > 8
                result['startup_evidence'] = {
                    'result_path': str(startup_result), 'sha256': file_digest(startup_result),
                    'coverage': report.get('coverage'), 'executables_match': executables_match,
                    'screenshots': screenshots}
                result['startup_passed'] = (executables_match and all(screenshots.values())
                                             and report.get('coverage', '').startswith('private headless compositor;'))
            elif operation in ('test', 'soroban'):
                result['test_evidence'] = self.test_counts(directory / 'build.log')
                result['test_count_passed'] = result['test_evidence']['passed'] > 0 and result['test_evidence']['failed'] == 0
                if operation == 'soroban':
                    result['test_count_passed'] = result['test_evidence']['passed'] == stage['expected_tests'] and result['test_evidence']['failed'] == 0
                if self.config['tool'] == 'buck2':
                    result['test_execution_passed'] = result.get('telemetry', {}).get('test_execution_count', 0) > 0
            result['passed'] = (rec['exit_status'] == 0 and not result.get('telemetry_error')
                                and all(c['passed'] for c in result.get('correctness', {}).values())
                                and result.get('test_count_passed', True)
                                and result.get('test_execution_passed', True)
                                and result.get('startup_passed', True)
                                and (build_info is None or build_info['exit_status'] == 0))
        except Exception as exc:
            result['postprocess_error'] = str(exc)
            result['passed'] = False
        save(directory / 'result.json', result)
        return result

    @staticmethod
    def test_counts(path):
        matches = re.findall(r'test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored', Path(path).read_text(errors='replace'))
        return {'passed': sum(int(x[0]) for x in matches), 'failed': sum(int(x[1]) for x in matches),
                'ignored': sum(int(x[2]) for x in matches), 'summaries': len(matches),
                'method': 'Rust libtest summaries; raw log retained; absence is zero, never assumed success'}

    def reference(self, ident):
        if not ident:
            return None
        path = self.root / 'stages' / ident / 'a' / 'result.json'
        if not path.exists():
            raise RuntimeError(f'missing original reference: {path}')
        result = load(path)
        if not result['passed']:
            raise RuntimeError(f'original reference failed: {path}; not replacing it with a warm retry')
        return result['correctness']

    def storage(self, stage, directory, root_key='roots'):
        config = self.config['storage']
        roots = []
        for value in expand(config[root_key], self.values()):
            if value.startswith('glob:'):
                import glob
                matches = sorted(glob.glob(value[5:]))
                if not matches:
                    raise RuntimeError(f'storage glob has no roots: {value}')
                roots.extend(Path(path).resolve() for path in matches)
            elif value.startswith('untracked:'):
                # Build state in each matching checkout: whatever git ignores or does not
                # track. Tracked source is the checkout's own size, kept apart by the gates.
                import glob
                matches = sorted(glob.glob(value[10:]))
                if not matches:
                    raise RuntimeError(f'storage glob has no roots: {value}')
                for tree in matches:
                    listing = subprocess.check_output(
                        ['git', '-C', tree, 'status', '--porcelain', '-z', '--ignored', '--untracked-files=normal'])
                    entries = [Path(tree).resolve() / entry[3:].decode().rstrip('/')
                               for entry in listing.split(b'\0') if entry[:3] in (b'?? ', b'!! ')]
                    # A symlink's target is either another root or outside the workload.
                    roots.extend(path for path in entries if not path.is_symlink())
            else:
                roots.append(Path(value).resolve())
        if any(not path.exists() for path in roots):
            raise RuntimeError(f'storage root missing; refusing unmatched denominator: {roots}')
        filesystems = {str(path): btrfs_filesystem_id(path) for path in roots}
        if len(set(filesystems.values())) != 1:
            raise RuntimeError(f'physical union roots span Btrfs filesystems: {filesystems}')
        command_key = 'immutable_command' if root_key == 'immutable_roots' and 'immutable_command' in config else 'command'
        command = expand(config[command_key], self.values()) + [str(path) for path in roots]
        rec = self.command(command, self.root, directory, 'physical-union')
        text = Path(rec['log']).read_text()
        match = re.search(r'(?m)^TOTAL\s+\S+\s+(\d+)\s+(\d+)\s+(\d+)\s*$', text)
        value = {'stage': stage['id'], 'roots': [str(path) for path in roots], 'command': rec,
                 'btrfs_filesystem_ids': filesystems,
                 'workload_boundary': {'completed_stages': self.completed(), 'profile': stage['profile'], 'trees': stage['trees']},
                 'method': 'single compression-aware compsize extent union; cache+outputs+mutable state, no summed component totals',
                 'passed': rec['exit_status'] == 0 and match is not None}
        try:
            provenance = json.loads(text.splitlines()[0])
        except (json.JSONDecodeError, IndexError):
            provenance = None
        if isinstance(provenance, dict) and provenance.get('roots'):
            value['expanded_measurement_roots'] = provenance['roots']
            value['measurement_inventory'] = provenance.get('inventory')
        if root_key == 'roots' and config.get('immutable_roots'):
            value['immutable_union'] = self.storage(stage, directory / 'immutable', 'immutable_roots')
            value['passed'] &= value['immutable_union']['passed']
        if match:
            value.update(physical_disk_bytes=int(match[1]), unique_uncompressed_bytes=int(match[2]), referenced_bytes=int(match[3]))
        if root_key == 'roots' and config.get('attribution_command'):
            # Per-root uncompressed extents (btrfs filesystem du --raw): explains growth, never gates it.
            rec = self.command(expand(config['attribution_command'], self.values()) + [str(path) for path in roots],
                               self.root, directory, 'attribution')
            rows = re.findall(r'(?m)^\s*(\d+)\s+(\d+)\s+(\d+)\s+(/.*)$', Path(rec['log']).read_text())
            value['attribution'] = {'command': rec, 'method': 'uncompressed FIEMAP extents; exclusive is fs-wide single-reference',
                                    'roots': {path: {'total': int(total), 'exclusive': int(exclusive), 'set_shared': int(shared)}
                                              for total, exclusive, shared, path in rows}}
        save(directory / 'storage.json', value)
        return value

    def completed(self):
        return sorted(path.parent.name for path in (self.root / 'stages').glob('*/complete.json'))

    def run(self, selected):
        self.prepare()
        stages = self.plan
        for index, stage in enumerate(stages):
            if selected and stage['id'] not in selected:
                continue
            missing_prior = [prior['id'] for prior in stages[:index]
                             if not (self.root / 'stages' / prior['id']).exists()]
            if missing_prior:
                raise RuntimeError(f'cannot relabel an unprimed build as {stage["id"]}; missing prior observations: {missing_prior}')
            directory = self.root / 'stages' / stage['id']
            if (directory / 'complete.json').exists():
                continue
            if directory.exists():
                # Never execute an interrupted stage again with warm state under its cold label.
                save(directory / ('interrupted-observation-' + str(time.time_ns()) + '.json'),
                     {'passed': False, 'reason': 'prior stage began but did not finish; original evidence preserved', 'time': time.time()})
                continue
            directory.mkdir(parents=True)
            save(directory / 'intent.json', {'stage': stage, 'start': time.time(), 'driver_sha256': self.driver_sha256})
            started = time.monotonic()
            try:
                operation = stage['operation']
                if operation in ('eviction', 'restart', 'restore_capacity', 'dedupe'):
                    service = self.config['service']
                    values = self.values()
                    tree = self.tree('a')
                    inventory_argv = expand(service['inventory'], values)
                    before = self.command(inventory_argv, tree, directory, 'inventory-before')
                    if before['exit_status']:
                        raise RuntimeError('cannot inventory owned service cache before operation')
                    before_files = load(before['log'])
                    policy_before = cache_inventory_policy(before_files)
                    save(directory / 'cache-policy-before.json', policy_before)
                    if not policy_before['passed']:
                        raise RuntimeError('owned cache exceeds configured policy bounds before operation')
                    metrics_before = self.metrics(values, directory, 'service-metrics-before')
                    rec = self.command(expand(service[operation], values), tree, directory, operation)
                    after = self.command(inventory_argv, tree, directory, 'inventory-after')
                    if after['exit_status']:
                        raise RuntimeError('cannot inventory owned service cache after operation')
                    after_files = load(after['log'])
                    policy_after = cache_inventory_policy(after_files)
                    save(directory / 'cache-policy-after.json', policy_after)
                    if before_files['roots'] != after_files['roots']:
                        raise RuntimeError('service operation changed cache roots; not matched eviction/restart evidence')
                    removed = sorted(set(before_files['files']) - set(after_files['files']))
                    retained = all(after_files['files'].get(key) == size for key, size in before_files['files'].items())
                    cache_proof = bool(removed) if operation == 'eviction' else bool(before_files['files']) and retained
                    cap = service.get('eviction_max_apparent_bytes')
                    if operation == 'eviction' and cap is not None:
                        cache_proof &= after_files['apparent_bytes'] <= cap
                    metrics_after = self.metrics(values, directory, 'service-metrics-after')
                    results = [{
                        'passed': rec['exit_status'] == 0 and cache_proof and policy_after['passed']
                                  and metrics_before.get('exit_status') == 0 and metrics_after.get('exit_status') == 0,
                        'service_operation': rec, 'supported_api': service.get(operation + '_api'),
                        'cache_inventory_before': before, 'cache_inventory_after': after,
                        'removed_cache_entries': removed, 'prior_cache_entries_retained': retained,
                        'cache_apparent_bytes_before': before_files['apparent_bytes'],
                        'cache_apparent_bytes_after': after_files['apparent_bytes'],
                        'cache_policy_before': policy_before, 'cache_policy_after': policy_after,
                        'metrics_before': metrics_before, 'metrics_after': metrics_after,
                        'note': 'inventory proves policy bounds and API removal/persistence; physical union measured separately'}]
                else:
                    preparation_start = time.monotonic()
                    prepared = {name: self.edit_source(name, stage) for name in stage['trees']}
                    save(directory / 'source-preparation.json', {
                        'wall_seconds': time.monotonic() - preparation_start,
                        'trees': prepared, 'excluded_from_build_timings': True})
                    start_barrier = threading.Barrier(len(stage['trees']))

                    def invoke(name):
                        try:
                            return self.trial(stage, name, directory / name, prepared[name], start_barrier)
                        except BaseException:
                            start_barrier.abort()
                            raise

                    with concurrent.futures.ThreadPoolExecutor(max_workers=len(stage['trees'])) as pool:
                        futures = [pool.submit(invoke, name) for name in stage['trees']]
                        results = [future.result() for future in futures]
                builds = [row['build'] for row in results if 'build' in row]
                makespan = (max(row['end_monotonic'] for row in builds) - min(row['start_monotonic'] for row in builds)
                            if builds else results[0]['service_operation']['wall_seconds'])
                launch_spread = (max(row['start_monotonic'] for row in builds) - min(row['start_monotonic'] for row in builds)
                                 if builds else None)
                summary = {'stage': stage, 'wall_makespan_seconds': makespan,
                           'orchestration_wall_seconds': time.monotonic() - started,
                           'concurrent_launch_spread_seconds': launch_spread,
                           'passed': all(row['passed'] for row in results), 'results': results}
                if stage.get('storage'):
                    summary['storage'] = self.storage(stage, directory)
                    summary['passed'] &= summary['storage']['passed']
                save(directory / 'complete.json', summary)
            except Exception:
                save(directory / 'failure.json', {'passed': False, 'traceback': traceback.format_exc(), 'time': time.time()})
                # Retain raw failures and continue independent observations; no automatic retries.
        return summarize(self.root)


def stage_trial_observations(root, stage, tool, completed=None):
    if completed is not None:
        return [trial for trial in completed['results'] if 'build' in trial]
    directory = root / 'stages' / stage['id']
    intent = directory / 'intent.json'
    recorded_stage = load(intent)['stage'] if intent.exists() else stage
    trials = []
    for name in recorded_stage['trees']:
        trial_directory = directory / name
        result = trial_directory / 'result.json'
        command = trial_directory / 'build.json'
        if result.exists():
            trials.append(load(result))
        elif command.exists():
            trials.append({
                'schema': SCHEMA, 'tool': tool, 'stage': recorded_stage, 'tree': name,
                'profile': recorded_stage['profile'], 'build': load(command), 'passed': False,
                'incomplete_observation': True, 'raw_build_record': str(command),
                'reason': 'Completed command is durable but trial postprocessing has no result; original timing retained, not replayed',
                'telemetry': {'compiler_executions': None},
            })
    return trials


def qualification_gates(root):
    """Prior-matrix thresholds: absent or unmatched evidence never passes.

    The storage plan keeps every storage, reuse and correctness gate. It takes
    one observation where the full plan takes five, and reports edit latency
    without gating on it."""
    config = load(root / 'config.json')
    kind = config.get('plan', 'full')
    planned = {stage['id'] for stage in plan(kind)}
    samples = 5 if kind == 'full' else 1
    stages = {path.parent.name: load(path) for path in (root / 'stages').glob('*/complete.json')}
    trials = [trial for stage in plan(kind)
              for trial in stage_trial_observations(root, stage, config['tool'], stages.get(stage['id']))]
    gates = {}

    def record(name, observed, passed, requirement, informational=False):
        status = 'unmeasured' if passed is None else ('pass' if passed else 'fail')
        gates[name] = {'status': 'informational' if informational and passed is not None else status,
                       'observed': observed, 'requirement': requirement}
        if informational:
            gates[name]['would_pass'] = passed

    noops = [trial for trial in trials if re.fullmatch('dev-noop-[1-5]', trial['stage']['id'])]
    times = [trial['build']['wall_seconds'] for trial in noops]
    median = statistics.median(times) if times else None
    record('noop', median, median <= 2 and all(trial['passed'] for trial in noops) if len(noops) == samples else None,
           f'{samples} noop(s), median <=2 seconds')
    fresh = [trial for trial in trials if re.fullmatch('dev-fresh-[bcde]', trial['stage']['id'])]
    compiles = [trial.get('telemetry', {}).get('compiler_executions') for trial in fresh]
    record('warm_identical_zero_compilers', compiles,
           all(value == 0 for value in compiles) and all(trial['passed'] for trial in fresh)
           if len(compiles) == 4 and None not in compiles else None,
           'four independent fresh worktrees, zero eligible compiler actions each')
    cold = [trial for trial in trials if trial['stage']['id'] == 'dev-cold']
    ratios = [trial['build']['wall_seconds'] / cold[0]['build']['wall_seconds'] for trial in fresh] if cold else []
    failed_warm_observation = any(not trial['passed'] for trial in cold + fresh)
    warm_wall_passed = (False if failed_warm_observation else
                        all(value <= .2 for value in ratios) if len(cold) == 1 and len(fresh) == 4 else None)
    record('warm_identical_wall', ratios, warm_wall_passed,
           'successful original cold and four successful identical fresh builds, each <=20% of original cold observation')
    baseline = load(config['cargo_baseline']) if config.get('cargo_baseline') else None
    if baseline:
        same_commit = baseline.get('commit') == config['commit']
        same_inputs = bool(config.get('comparison_identity')) and baseline.get('comparison_identity') == config['comparison_identity']
        if not (same_commit or same_inputs) or baseline.get('features') != config['profiles']['dev']['features']:
            baseline = None
    for edit in ('leaf', 'shared', 'icon'):
        selected = [trial for trial in trials if re.fullmatch(f'dev-{edit}-[1-5]', trial['stage']['id'])]
        values = [trial['build']['wall_seconds'] for trial in selected]
        measured = statistics.median(values) if values else None
        cargo = baseline.get(edit + '_median_seconds') if baseline else None
        threshold = max(2., 1.5 * cargo) if cargo is not None else None
        record(edit + '_edit', {'median_seconds': measured, 'cargo_median_seconds': cargo, 'threshold_seconds': threshold},
               measured <= threshold and all(trial['passed'] for trial in selected)
               if len(selected) == samples and threshold is not None else None,
               f'{samples} edit(s), median <=max(2 seconds,1.5 * matched frozen-source/features Cargo median)',
               informational=samples != 5)
    # Match the retained cache workload AFTER edits/revert, not the smaller cold cache.
    sharing_base = stages.get('dev-revert', {}).get('storage', {})
    denominator = sharing_base.get('immutable_union', {}).get('physical_disk_bytes')
    previous = sharing_base.get('physical_disk_bytes')
    sharing_storage_valid = sharing_base.get('passed', False)
    deltas, boundaries = [], []
    for tree in 'bcde':
        storage = stages.get('dev-fresh-' + tree, {}).get('storage', {})
        current = storage.get('physical_disk_bytes')
        sharing_storage_valid &= storage.get('passed', False)
        if previous is not None and current is not None:
            deltas.append(current - previous)
            boundaries.append(storage.get('workload_boundary'))
        previous = current
    limit = .05 * denominator + 32 * 1024 * 1024 if denominator is not None else None
    record('physical_sharing', {'incremental_union_bytes': deltas, 'one_tree_immutable_union_bytes': denominator,
                               'per_tree_limit_bytes': limit, 'baseline': 'dev-revert', 'workload_boundaries': boundaries},
           sharing_storage_valid and all(value <= limit for value in deltas) if len(deltas) == 4 and limit is not None else None,
           'each extra worktree <=5% matched one-tree immutable payload +32 MiB metadata; cache+all outputs union')
    churn = [] if kind != 'full' else [stages.get(f'dev-churn-{n}', {}).get('storage', {}).get('physical_disk_bytes') for n in range(1, 11)]
    budget = config.get('storage', {}).get('max_active_union_bytes')
    churn_storage_valid = all(stages.get(f'dev-churn-{n}', {}).get('storage', {}).get('passed', False)
                              for n in range(1, 11))
    record('observed_churn_budget', {'churn_union_bytes': churn, 'configured_budget_bytes': budget},
           churn_storage_valid and all(value <= budget for value in churn) if budget and churn and None not in churn else None,
           'ten observed churn unions fit the configured budget; this does not prove automatic total-state enforcement')
    record('automatic_total_state_bound', None, None,
           'unmeasured: cache caps and finite churn observations do not demonstrate automatic enforcement over all owned state')
    for name, required in (
        ('restart', ['service-restart', 'dev-restart-edited', 'dev-restart-original']),
        ('eviction_recovery', ['service-eviction', 'dev-post-eviction-original', 'dev-post-eviction-known']),
        ('primed_output_loss', ['dev-primed-output-loss']),
    ):
        required = [item for item in required if item in planned]
        record(name, required, all(stages[item]['passed'] for item in required) if all(item in stages for item in required) else None,
               'supported owned service operation and successful exact-source artifact/behavior checks')
    record('correctness', {'observations': len(trials), 'failed': sum(not trial['passed'] for trial in trials)},
           all(trial['passed'] for trial in trials) if trials else None, 'all raw client and correctness observations pass')
    return gates


def summarize(root):
    root = Path(root)
    config = load(root / 'config.json')
    tool = config['tool']
    stages = plan(config.get('plan', 'full'))
    rows = []
    interrupted = []
    retained_partial_trials = []
    groups = {}
    for stage in stages:
        directory = root / 'stages' / stage['id']
        complete = directory / 'complete.json'
        if complete.exists():
            value = load(complete)
            rows.append({'id': stage['id'], 'passed': value['passed'], 'makespan_seconds': value['wall_makespan_seconds']})
            trials = value['results']
        else:
            if directory.exists():
                interrupted.append(stage['id'])
            trials = stage_trial_observations(root, stage, tool)
            retained_partial_trials.extend({
                'record': trial.get('raw_build_record', str(directory / trial['tree'] / 'result.json')),
                'trial_postprocessing_complete': not trial.get('incomplete_observation', False),
            } for trial in trials)
        for trial in trials:
            if 'build' not in trial:
                continue
            key = re.sub(r'-[1-9][0-9]*$', '', stage['id'])
            key = f'{trial["tool"]}/{stage["profile"]}/{key}/j{stage["jobs"]}'
            groups.setdefault(key, []).append(trial)
    summary = {'schema': SCHEMA, 'root': str(root), 'generated_unix': time.time(), 'stages': rows,
               'raw_failed_stages': sum(not row['passed'] for row in rows) + len(interrupted),
               'interrupted_or_harness_failed_stages': interrupted,
               'durable_observations_from_incomplete_stages': retained_partial_trials,
               'missing_stages': [stage['id'] for stage in stages if not (root / 'stages' / stage['id']).exists()],
               'groups': {}, 'gates': {'all_required_stages_passed': len(rows) == len(stages) and all(row['passed'] for row in rows),
                                     'host_load_gating': False, 'global_host_gc': False}}
    summary['raw_failed_build_commands'] = sum(
        load(path)['exit_status'] != 0 for path in (root / 'stages').glob('*/*/build.json'))
    summary['raw_failed_correctness_observations'] = sum(
        not load(path)['passed'] for path in (root / 'stages').glob('*/*/result.json'))
    summary['observed_failed_native_attempts'] = sum(
        trial.get('telemetry', {}).get('failed_native_attempts', 0)
        for trials in groups.values() for trial in trials)
    summary['qualification_gates'] = qualification_gates(root)
    for key, trials in groups.items():
        times = [row['build']['wall_seconds'] for row in trials]
        passed_times = [row['build']['wall_seconds'] for row in trials if row['passed']]
        summary['groups'][key] = {'observations': len(trials), 'raw_failures': sum(not row['passed'] for row in trials),
                                  'all_wall_median': statistics.median(times), 'all_wall_range': [min(times), max(times)],
                                  'passed_wall_median': statistics.median(passed_times) if passed_times else None,
                                  'passed_wall_range': [min(passed_times), max(passed_times)] if passed_times else None,
                                  'compiler_executions': [row.get('telemetry', {}).get('compiler_executions') for row in trials],
                                  'final_attempt_compiler_executions': [row.get('telemetry', {}).get('final_compiler_executions') for row in trials],
                                  'observed_failed_native_attempts': [row.get('telemetry', {}).get('failed_native_attempts') for row in trials]}
    save(root / 'summaries' / (str(time.time_ns()) + '.json'), summary)
    return summary


def validate(config, full):
    required = ('tool', 'repo', 'commit', 'root', 'client', 'profiles', 'artifacts')
    for key in required:
        if key not in config:
            raise ValueError(f'missing configuration field: {key}')
    for profile in PROFILES:
        value = config['profiles'][profile]
        if 'features' not in value or not value.get('targets'):
            raise ValueError(f'{profile}: explicit feature identity and nonempty real targets required')
    for kind in ('leaf', 'shared', 'icon'):
        selector = config['artifacts'].get(kind, {})
        if not selector.get('target') and not all(selector.get(profile, {}).get('target') for profile in PROFILES):
            raise ValueError(f'{kind}: declared artifact target required for every profile')
    if config['tool'] == 'buck2' and not config.get('buck_telemetry_flags'):
        raise ValueError('Buck native event log/build report flags required')
    kind = config.get('plan', 'full')
    if kind not in PLANS:
        raise ValueError(f'plan must be one of {PLANS}')
    if full:
        if config['tool'] == 'buck2' and not config.get('buck_engine'):
            raise ValueError('Buck native engine argv required for same-path output-loss recovery')
        if len(config.get('test_groups', [])) != 9:
            raise ValueError('full matrix requires exactly nine explicit package test groups')
        runtime_key = 'runtime_targets' if config['tool'] == 'buck2' else 'runtime_artifacts'
        startup = ('startup', 'startup_result', runtime_key) if kind == 'full' else ()
        for key in ('soroban_tests', 'correctness_behavior', 'storage', 'nix_identity', 'variants') + startup:
            if not config.get(key):
                raise ValueError(f'{kind} plan requires {key}')
        if startup and set(config[runtime_key]) != {'compositor', 'shell', 'ctl', 'koe'}:
            raise ValueError('private startup requires exact compositor/shell/ctl/koe artifact identities')
        if not config['storage'].get('immutable_roots'):
            raise ValueError('full matrix requires separately measured matched immutable union roots')
        for key in ('restart', 'eviction', 'restore_capacity', 'dedupe', 'restart_api', 'eviction_api',
                    'restore_capacity_api', 'dedupe_api', 'identity', 'inventory', 'metrics'):
            if not config.get('service', {}).get(key):
                raise ValueError(f'full matrix requires supported service {key}')


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    listing = sub.add_parser('plan', help='print exact required scenarios as JSON')
    listing.add_argument('--kind', choices=PLANS, default='full')
    execute = sub.add_parser('run', help='run/resume frozen configuration; no automatic reruns')
    execute.add_argument('config', type=Path)
    execute.add_argument('--stage', action='append', help='execute/resume explicit ordered matrix stages')
    execute.add_argument('--go', action='store_true', help='acknowledge parent freeze/GO for full matrix')
    smoke = sub.add_parser('smoke', help='isolated real client/artifact exercise, never a full-matrix observation')
    smoke.add_argument('config', type=Path)
    smoke.add_argument('--kind', choices=('leaf', 'shared', 'icon'), default='shared')
    smoke.add_argument('--profile', choices=PROFILES, default='dev')
    smoke.add_argument('--edit', action='store_true', help='exercise surgical source edit and correctness check')
    smoke.add_argument('--target', action='append', help='explicit target for this targeted smoke only')
    report = sub.add_parser('summarize')
    report.add_argument('root', type=Path)
    args = parser.parse_args()
    if args.command == 'plan':
        value = plan(args.kind)
    elif args.command == 'summarize':
        value = summarize(args.root)
    elif args.command == 'smoke':
        config = load(args.config)
        validate(config, False)
        bench = Bench(config)
        bench.prepare()
        ident = 'smoke-' + str(time.time_ns())
        stage = {'id': ident, 'trees': ['a'], 'profile': args.profile, 'jobs': 4,
                 'edits': {args.kind: ident} if args.edit else {},
                 'operation': 'build', 'checks': [args.kind]}
        if args.target:
            stage['targets'] = args.target
        trial = bench.trial(stage, 'a', bench.root / 'smoke' / ident)
        value = {'result_path': str(bench.root / 'smoke' / ident / 'result.json'),
                 'passed': trial['passed'], 'compiler_executions': trial.get('telemetry', {}).get('compiler_executions'),
                 'raw_failed_stages': int(not trial['passed']),
                 'qualification': 'targeted driver smoke only; not matrix or performance evidence'}
    else:
        config = load(args.config)
        if not args.go:
            parser.error('matrix stages require explicit --go after qualified implementation freeze; use smoke before GO')
        if args.stage and set(args.stage) - {stage['id'] for stage in plan(config.get('plan', 'full'))}:
            parser.error('unknown stage')
        validate(config, True)
        value = Bench(config).run(set(args.stage or []))
    print(json.dumps(value, indent=2, sort_keys=True))
    if args.command == 'plan':
        return 0
    return int(value['raw_failed_stages'] != 0)


if __name__ == '__main__':
    sys.exit(main())
