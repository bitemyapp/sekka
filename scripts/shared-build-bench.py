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
service (identity, metrics/restart/eviction argv), storage (roots, command),
test_groups (nine named argv suffixes), soroban_tests, startup (argv),
nix_identity (evidence paths), variants (client/service/source identity metadata).
Artifact selectors have target, optional file_regex and member (e.g. icons.rs).
Storage command must emit compsize's compression-aware TOTAL row and receives
ALL storage roots in ONE invocation. No component sums or df deltas are used.
All placeholders are whole argument/substring replacements: {tree}, {root},
{profile}, {jobs}, {trial}, {state}, {output}, {python}. See plan --help.
"""
import argparse
import concurrent.futures
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


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def save(path, value):
    """Create-only observations: collisions are errors, including on resume."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x') as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())


def load(path):
    return json.loads(Path(path).read_text())


def plan():
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
    add('final-original', reference='dev-cold', storage=True)
    return stages


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
    text = Path(path).read_text()
    decoder = json.JSONDecoder()
    pos = 0
    while pos < len(text):
        while pos < len(text) and text[pos].isspace():
            pos += 1
        if pos == len(text):
            break
        value, pos = decoder.raw_decode(text, pos)
        yield value


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
        for action in json_stream(execution):
            runner = action.get('runner', 'unknown')
            runners[runner] = runners.get(runner, 0) + 1
            if action.get('cacheHit'):
                hits += 1
            else:
                mnemonic = action.get('mnemonic', 'unknown')
                actions[mnemonic] = actions.get(mnemonic, 0) + 1
            keys.append({k: action[k] for k in ('digest', 'mnemonic', 'targetLabel', 'runner', 'cacheHit') if k in action})
        result.update(executed_mnemonics=actions, runners=runners, remote_or_disk_hits=hits,
                      compiler_executions=sum(n for key, n in actions.items() if 'Rustc' in key or key == 'CppCompile'),
                      action_keys=keys)
    result['action_cpu_seconds'] = None
    result['action_cpu_reason'] = 'not inferred from client CPU or summed wall time; native raw telemetry retained'
    return result


def buck_telemetry(path):
    """Buck2 2026-10-15 data.proto ActionExecutionKind, not console heuristics.

    Schema: https://github.com/facebook/buck2/blob/2026-10-15/app/buck2_data/data.proto
    cargo_build/cargo_test each invoke one rustc; native subprocesses inside
    run-custom-build are NOT guessed from the outer action count.
    """
    kinds = {0: 'unset', 1: 'local', 2: 'remote', 3: 'action_cache', 4: 'simple',
             6: 'deferred', 7: 'local_dep_file', 8: 'local_worker',
             9: 'remote_dep_file_cache', 10: 'local_action_cache',
             11: 'remote_worker', 12: 'remote_served_from_cache', 13: 'remote_deduplicated'}
    result = {'compiler_executions': 0, 'execution_kinds': {}, 'executed_categories': {},
              'action_keys': [], 'failed_actions': 0, 'native_compiler_subprocesses': None,
              'compiler_count_definition': 'one rustc per executed cargo_build/cargo_test action; buildscript native subprocesses unmeasured',
              'native_event_json': str(path), 'snapshots': []}
    commands_complete = False
    for row in json_stream(path):
        event = row.get('Event', {}).get('data', {})
        end = event.get('SpanEnd', {}).get('data', {})
        if 'Command' in end:
            commands_complete = True
        snapshot = event.get('Instant', {}).get('data', {}).get('Snapshot')
        if snapshot:
            result['snapshots'].append(snapshot)
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
        if action.get('failed'):
            result['failed_actions'] += 1
        digests = []
        for command in action.get('commands', []):
            variants = command.get('details', {}).get('command_kind', {}).get('command', {})
            for detail in variants.values():
                if isinstance(detail, dict) and detail.get('action_digest'):
                    digests.append(detail['action_digest'])
        result['action_keys'].append({'key': action.get('key'), 'category': category,
                                      'execution_kind': name, 'action_digests': digests,
                                      'commands': action.get('commands', []),
                                      'cache_upload_result': action.get('cache_upload_result')})
    result['command_end_observed'] = commands_complete
    if not commands_complete:
        result['compiler_executions'] = None
    # Endpoints suffice for daemon metrics; all original samples remain in JSON.
    result['snapshots'] = result['snapshots'][:1] + result['snapshots'][-1:]
    return result


class Bench:
    def __init__(self, config):
        self.config = config
        self.root = Path(config['root']).resolve()
        self.repo = Path(config['repo']).resolve()
        self.commit = config['commit']
        if not re.fullmatch('[0-9a-f]{40}', self.commit):
            raise ValueError('commit must be the frozen full 40-character SHA')
        if config['tool'] not in ('bazel', 'buck2'):
            raise ValueError('tool must be bazel or buck2')
        self.env = dict(os.environ, **config.get('environment', {}))
        self.fingerprint = digest(canonical(config))
        self.root.mkdir(parents=True, exist_ok=True)
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
            save(self.root / 'plan.json', plan())
        self.lock = (self.root / '.lock').open('a')
        fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.original = {}
        for relative in (LEAF, SHARED):
            self.original[relative] = subprocess.check_output(['git', '-C', str(self.repo), 'show', f'{self.commit}:{relative}'])
        self.write_lock = threading.Lock()

    def values(self, tree='a', profile='dev', jobs=16, trial='setup'):
        return {'tree': self.root / 'trees' / tree, 'root': self.root, 'profile': profile,
                'jobs': jobs, 'trial': self.root / 'trials' / trial, 'state': self.root / 'state' / tree,
                'output': self.root / 'state' / tree / 'outputs', 'python': sys.executable}

    def command(self, argv, cwd, directory, name, *, timeout=None, env=None):
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=True)
        log = directory / (name + '.log')
        start = time.time()
        mono = time.monotonic()
        record = {'argv': argv, 'cwd': str(cwd), 'start_unix': start,
                  'load_before': list(os.getloadavg()), 'log': str(log)}
        # The intent is durable before spawning, so interruption remains visible.
        save(directory / (name + '.intent.json'), record)
        try:
            with log.open('xb') as stream:
                proc = subprocess.Popen(argv, cwd=cwd, env=env or self.env, stdout=stream,
                                        stderr=subprocess.STDOUT, start_new_session=True)
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
        record.update(exit_status=status, wall_seconds=time.monotonic() - mono,
                      end_unix=time.time(), load_after=list(os.getloadavg()))
        save(directory / (name + '.json'), record)
        return record

    def tree(self, name):
        path = self.root / 'trees' / name
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

    def client(self, values, operation, extra=()):
        config = self.config
        base = expand(config['client'], values)
        profile = str(values['profile'])
        if config['tool'] == 'bazel':
            base += ['--profile', profile, '--output-base', str(values['output']),
                     '--disk-cache', str(self.root / 'cache' / 'bazel')]
            base += expand(config.get('client_flags', []), values)
            return base + [operation] + list(extra)
        # Isolation participates in Buck action paths: keep it identical across
        # fresh trees, machines, and output-loss recovery, not random per run.
        isolation = config.get('buck_isolation', 'shared-bench')
        base += ['--profile', profile, '--isolation-dir', isolation, '-j', str(values['jobs'])]
        base += expand(config.get('client_flags', []), values)
        return base + [operation] + list(extra)

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
        source['nix_identity_files'] = [
            {'path': str(path), 'sha256': file_digest(path)}
            for path in self.config.get('nix_identity', [])]
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
        temporary = old_state.with_suffix('.tmp')
        temporary.write_bytes(canonical(new_state))
        temporary.replace(old_state)
        return markers, changes

    def artifact(self, kind, values, directory):
        selector = self.config['artifacts'][kind]
        selector = selector.get(str(values['profile']), selector)
        target = selector['target']
        if self.config['tool'] == 'bazel':
            materialize = self.client(values, 'build', ['--remote_download_outputs=all', target])
            rec = self.command(materialize, values['tree'], directory, kind + '-materialize')
            if rec['exit_status']:
                raise RuntimeError(f'artifact materialization failed: {kind}')
            argv = self.client(values, 'cquery', ['--output=files', target])
            query = self.command(argv, values['tree'], directory, kind + '-declared')
            if query['exit_status']:
                raise RuntimeError(f'artifact declaration query failed: {kind}')
            lines = Path(query['log']).read_text().splitlines()
            candidates = [Path(values['tree']) / line for line in lines if line.startswith(('bazel-out/', '/'))]
        else:
            argv = self.client(values, 'outputs', [target])
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
            # Bounded known marker language; trailing -END prevents 1 matching 10.
            found = sorted(set(x.decode() for x in re.findall(rb'sekka-bench-[a-zA-Z0-9-]+-END', data)))
            expected = [marker] if marker else []
            check = {'artifact': str(path), 'sha256': digest(data), 'bytes': len(data),
                     'markers': found, 'expected': expected, 'passed': found == expected}
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
            rec = self.command(expand(self.config['correctness_behavior'], values),
                               values['tree'], directory, 'original-behavior')
            counts = self.test_counts(rec['log'])
            checks['original_behavior'] = {
                'command': rec, 'test_counts': counts,
                'passed': rec['exit_status'] == 0 and counts['passed'] == 11 and counts['failed'] == 0}
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

    def trial(self, stage, name, directory):
        values = self.values(name, stage['profile'], stage['jobs'], stage['id'] + '-' + name)
        tree = self.tree(name)
        markers, changes = self.edit_source(name, stage)
        if stage.get('output_loss'):
            self.lose_outputs(values, directory)
        profile = self.config['profiles'][stage['profile']]
        targets = profile['targets']
        operation = stage['operation']
        extra = list(targets)
        if operation == 'test':
            extra = self.config['test_groups'][stage['test_group']]['args']
        elif operation == 'soroban':
            extra = self.config['soroban_tests']['args']
        prefix = directory / 'native'
        if operation == 'startup':
            argv = expand(self.config['startup'], values)
        else:
            native_operation = 'test' if operation in ('test', 'soroban') else 'build'
            if self.config['tool'] == 'bazel':
                telemetry_flags = [f'--jobs={stage["jobs"]}', f'--build_event_json_file={prefix}.bep.json',
                                   f'--execution_log_json_file={prefix}.exec.json', f'--profile={prefix}.profile.gz']
                if native_operation == 'test':
                    telemetry_flags += ['--nocache_test_results', '--test_output=all']
            else:
                telemetry_flags = expand(self.config['buck_telemetry_flags'], dict(values, telemetry=prefix))
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
        rec = self.command(argv, tree, directory, 'build')
        result = {'schema': SCHEMA, 'stage': stage, 'tree': name, 'tool': self.config['tool'],
                  'profile': stage['profile'], 'features': profile['features'], 'source': source,
                  'host': socket.gethostname(), 'service': self.config.get('service', {}).get('identity'),
                  'variants': self.config.get('variants', []), 'build': rec,
                  'metrics_before': before, 'metrics_after': self.metrics(values, directory, 'metrics-after')}
        try:
            if self.config['tool'] == 'bazel':
                result['telemetry'] = bazel_telemetry(prefix)
            else:
                event_path = Path(str(prefix) + '.events.json-lines.gz')
                if event_path.exists():
                    engine = self.config.get('buck_engine')
                    if not engine and self.env.get('SEKKA_BUCK2_BIN'):
                        engine = [self.env['SEKKA_BUCK2_BIN']]
                    if not engine:
                        raise RuntimeError('buck_engine argv required to decode native events')
                    decoded = self.command(expand(engine, values) + ['log', 'show', str(event_path)],
                                           tree, directory, 'telemetry')
                    result['telemetry_collection'] = decoded
                    if decoded['exit_status']:
                        raise RuntimeError('native Buck event decoding failed')
                    result['telemetry'] = buck_telemetry(decoded['log'])
                else:
                    result['telemetry'] = {'compiler_executions': None, 'native_event_log_missing': str(event_path)}
        except Exception as exc:
            result['telemetry_error'] = str(exc)
        try:
            if rec['exit_status'] == 0 and operation == 'build':
                reference = self.reference(stage.get('reference'))
                result['correctness'] = self.check(markers, values, directory, reference,
                                                   stage.get('checks', ('leaf', 'shared', 'icon')))
            elif rec['exit_status'] == 0 and operation in ('test', 'soroban'):
                result['test_evidence'] = self.test_counts(directory / 'build.log')
                result['test_count_passed'] = result['test_evidence']['summaries'] > 0 and result['test_evidence']['failed'] == 0
                if operation == 'soroban':
                    result['test_count_passed'] = result['test_evidence']['passed'] == stage['expected_tests'] and result['test_evidence']['failed'] == 0
            result['passed'] = (rec['exit_status'] == 0 and not result.get('telemetry_error')
                                and all(c['passed'] for c in result.get('correctness', {}).values())
                                and result.get('test_count_passed', True))
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
            else:
                roots.append(Path(value).resolve())
        if any(not path.exists() for path in roots):
            raise RuntimeError(f'storage root missing; refusing unmatched denominator: {roots}')
        devices = {path.stat().st_dev for path in roots}
        if len(devices) != 1:
            raise RuntimeError('physical union roots span filesystems; configure one union per filesystem')
        command = expand(config['command'], self.values()) + [str(path) for path in roots]
        rec = self.command(command, self.root, directory, 'physical-union')
        text = Path(rec['log']).read_text()
        match = re.search(r'(?m)^TOTAL\s+\S+\s+(\d+)\s+(\d+)\s+(\d+)\s*$', text)
        value = {'stage': stage['id'], 'roots': [str(path) for path in roots], 'command': rec,
                 'workload_boundary': {'completed_stages': self.completed(), 'profile': stage['profile'], 'trees': stage['trees']},
                 'method': 'single compression-aware compsize extent union; cache+outputs+mutable state, no summed component totals',
                 'passed': rec['exit_status'] == 0 and match is not None}
        if root_key == 'roots' and config.get('immutable_roots'):
            value['immutable_union'] = self.storage(stage, directory / 'immutable', 'immutable_roots')
            value['passed'] &= value['immutable_union']['passed']
        if match:
            value.update(physical_disk_bytes=int(match[1]), unique_uncompressed_bytes=int(match[2]), referenced_bytes=int(match[3]))
        save(directory / 'storage.json', value)
        return value

    def completed(self):
        return sorted(path.parent.name for path in (self.root / 'stages').glob('*/complete.json'))

    def run(self, selected):
        self.prepare()
        for stage in plan():
            if selected and stage['id'] not in selected:
                continue
            directory = self.root / 'stages' / stage['id']
            if (directory / 'complete.json').exists():
                continue
            if directory.exists():
                # Never execute an interrupted stage again with warm state under its cold label.
                save(directory / ('interrupted-observation-' + str(time.time_ns()) + '.json'),
                     {'passed': False, 'reason': 'prior stage began but did not finish; original evidence preserved', 'time': time.time()})
                continue
            directory.mkdir(parents=True)
            save(directory / 'intent.json', {'stage': stage, 'start': time.time()})
            started = time.monotonic()
            try:
                operation = stage['operation']
                if operation in ('eviction', 'restart'):
                    service = self.config['service']
                    rec = self.command(expand(service[operation], self.values()), self.tree('a'), directory, operation)
                    results = [{'passed': rec['exit_status'] == 0, 'service_operation': rec,
                                'supported_api': service.get(operation + '_api')}]
                else:
                    with concurrent.futures.ThreadPoolExecutor(max_workers=len(stage['trees'])) as pool:
                        futures = [pool.submit(self.trial, stage, name, directory / name) for name in stage['trees']]
                        results = [future.result() for future in futures]
                summary = {'stage': stage, 'wall_makespan_seconds': time.monotonic() - started,
                           'passed': all(row['passed'] for row in results), 'results': results}
                if stage.get('storage'):
                    summary['storage'] = self.storage(stage, directory)
                    summary['passed'] &= summary['storage']['passed']
                save(directory / 'complete.json', summary)
            except Exception:
                save(directory / 'failure.json', {'passed': False, 'traceback': traceback.format_exc(), 'time': time.time()})
                # Retain raw failures and continue independent observations; no automatic retries.
        return summarize(self.root)


def qualification_gates(root):
    """Prior-matrix thresholds: absent or unmatched evidence never passes."""
    config = load(root / 'config.json')
    stages = {path.parent.name: load(path) for path in (root / 'stages').glob('*/complete.json')}
    trials = [trial for stage in stages.values() for trial in stage['results'] if 'build' in trial]
    gates = {}

    def record(name, observed, passed, requirement):
        gates[name] = {'status': 'unmeasured' if passed is None else ('pass' if passed else 'fail'),
                       'observed': observed, 'requirement': requirement}

    noops = [trial for trial in trials if re.fullmatch('dev-noop-[1-5]', trial['stage']['id'])]
    times = [trial['build']['wall_seconds'] for trial in noops]
    median = statistics.median(times) if times else None
    record('noop', median, median <= 2 and all(trial['passed'] for trial in noops) if len(noops) == 5 else None,
           'five noops, median <=2 seconds')
    fresh = [trial for trial in trials if re.fullmatch('dev-fresh-[bcde]', trial['stage']['id'])]
    compiles = [trial.get('telemetry', {}).get('compiler_executions') for trial in fresh]
    record('warm_identical_zero_compilers', compiles,
           all(value == 0 for value in compiles) and all(trial['passed'] for trial in fresh)
           if len(compiles) == 4 and None not in compiles else None,
           'four independent fresh worktrees, zero eligible compiler actions each')
    cold = stages.get('dev-cold', {}).get('results', [])
    ratios = [trial['build']['wall_seconds'] / cold[0]['build']['wall_seconds'] for trial in fresh] if cold else []
    record('warm_identical_wall', ratios, all(value <= .2 for value in ratios) if len(ratios) == 4 else None,
           'each identical fresh build <=20% of original cold observation')
    baseline = load(config['cargo_baseline']) if config.get('cargo_baseline') else None
    if baseline and (baseline.get('commit') != config['commit'] or baseline.get('features') != config['profiles']['dev']['features']):
        baseline = None
    for kind in ('leaf', 'shared', 'icon'):
        selected = [trial for trial in trials if re.fullmatch(f'dev-{kind}-[1-5]', trial['stage']['id'])]
        values = [trial['build']['wall_seconds'] for trial in selected]
        measured = statistics.median(values) if values else None
        cargo = baseline.get(kind + '_median_seconds') if baseline else None
        threshold = max(2., 1.5 * cargo) if cargo is not None else None
        record(kind + '_edit', {'median_seconds': measured, 'cargo_median_seconds': cargo, 'threshold_seconds': threshold},
               measured <= threshold and all(trial['passed'] for trial in selected)
               if len(selected) == 5 and threshold is not None else None,
               'five edits, median <=max(2 seconds,1.5 * matched frozen-source/features Cargo median)')
    # Match the retained cache workload AFTER edits/revert, not the smaller cold cache.
    sharing_base = stages.get('dev-revert', {}).get('storage', {})
    denominator = sharing_base.get('immutable_union', {}).get('physical_disk_bytes')
    previous = sharing_base.get('physical_disk_bytes')
    deltas, boundaries = [], []
    for tree in 'bcde':
        storage = stages.get('dev-fresh-' + tree, {}).get('storage', {})
        current = storage.get('physical_disk_bytes')
        if previous is not None and current is not None:
            deltas.append(current - previous)
            boundaries.append(storage.get('workload_boundary'))
        previous = current
    limit = .05 * denominator + 32 * 1024 * 1024 if denominator is not None else None
    record('physical_sharing', {'incremental_union_bytes': deltas, 'one_tree_immutable_union_bytes': denominator,
                               'per_tree_limit_bytes': limit, 'baseline': 'dev-revert', 'workload_boundaries': boundaries},
           all(value <= limit for value in deltas) if len(deltas) == 4 and limit is not None else None,
           'each extra worktree <=5% matched one-tree immutable payload +32 MiB metadata; cache+all outputs union')
    churn = [stages.get(f'dev-churn-{n}', {}).get('storage', {}).get('physical_disk_bytes') for n in range(1, 11)]
    budget = config.get('storage', {}).get('max_active_union_bytes')
    record('bounded_total_state', {'churn_union_bytes': churn, 'configured_budget_bytes': budget},
           all(value <= budget for value in churn) if budget and None not in churn else None,
           'ten churn observations fit explicit active-union budget; cache cap alone is not total-state cap')
    for name, required in (
        ('restart', ['service-restart', 'dev-restart-edited', 'dev-restart-original']),
        ('eviction_recovery', ['service-eviction', 'dev-post-eviction-original', 'dev-post-eviction-known']),
        ('primed_output_loss', ['dev-primed-output-loss']),
    ):
        record(name, required, all(stages[item]['passed'] for item in required) if all(item in stages for item in required) else None,
               'supported owned service operation and successful exact-source artifact/behavior checks')
    record('correctness', {'observations': len(trials), 'failed': sum(not trial['passed'] for trial in trials)},
           all(trial['passed'] for trial in trials) if trials else None, 'all raw client and correctness observations pass')
    return gates


def summarize(root):
    root = Path(root)
    rows = []
    interrupted = []
    groups = {}
    for stage in plan():
        directory = root / 'stages' / stage['id']
        complete = directory / 'complete.json'
        if not complete.exists():
            if directory.exists():
                interrupted.append(stage['id'])
            continue
        value = load(complete)
        rows.append({'id': stage['id'], 'passed': value['passed'], 'makespan_seconds': value['wall_makespan_seconds']})
        for trial in value['results']:
            if 'build' not in trial:
                continue
            key = re.sub(r'-[1-9][0-9]*$', '', stage['id'])
            key = f'{trial["tool"]}/{stage["profile"]}/{key}/j{stage["jobs"]}'
            groups.setdefault(key, []).append(trial)
    summary = {'schema': SCHEMA, 'root': str(root), 'generated_unix': time.time(), 'stages': rows,
               'raw_failed_stages': sum(not row['passed'] for row in rows) + len(interrupted),
               'interrupted_or_harness_failed_stages': interrupted,
               'missing_stages': [stage['id'] for stage in plan() if not (root / 'stages' / stage['id']).exists()],
               'groups': {}, 'gates': {'all_required_stages_passed': len(rows) == len(plan()) and all(row['passed'] for row in rows),
                                     'host_load_gating': False, 'global_host_gc': False}}
    for key, trials in groups.items():
        times = [row['build']['wall_seconds'] for row in trials]
        passed_times = [row['build']['wall_seconds'] for row in trials if row['passed']]
        summary['groups'][key] = {'observations': len(trials), 'raw_failures': sum(not row['passed'] for row in trials),
                                  'all_wall_median': statistics.median(times), 'all_wall_range': [min(times), max(times)],
                                  'passed_wall_median': statistics.median(passed_times) if passed_times else None,
                                  'passed_wall_range': [min(passed_times), max(passed_times)] if passed_times else None,
                                  'compiler_executions': [row.get('telemetry', {}).get('compiler_executions') for row in trials]}
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
    if full:
        if config['tool'] == 'buck2' and not config.get('buck_engine'):
            raise ValueError('Buck native engine argv required for same-path output-loss recovery')
        if len(config.get('test_groups', [])) != 9:
            raise ValueError('full matrix requires exactly nine explicit package test groups')
        for key in ('soroban_tests', 'startup', 'storage', 'nix_identity', 'variants'):
            if not config.get(key):
                raise ValueError(f'full matrix requires {key}')
        for key in ('restart', 'eviction', 'restart_api', 'eviction_api', 'identity'):
            if not config.get('service', {}).get(key):
                raise ValueError(f'full matrix requires supported service {key}')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('plan', help='print exact required scenarios as JSON')
    execute = sub.add_parser('run', help='run/resume frozen configuration; no automatic reruns')
    execute.add_argument('config', type=Path)
    execute.add_argument('--stage', action='append', help='targeted smoke stage; omitted means full matrix')
    execute.add_argument('--go', action='store_true', help='acknowledge parent freeze/GO for full matrix')
    smoke = sub.add_parser('smoke', help='isolated real client/artifact exercise, never a full-matrix observation')
    smoke.add_argument('config', type=Path)
    smoke.add_argument('--kind', choices=('leaf', 'shared', 'icon'), default='shared')
    smoke.add_argument('--edit', action='store_true', help='exercise surgical source edit and correctness check')
    report = sub.add_parser('summarize')
    report.add_argument('root', type=Path)
    args = parser.parse_args()
    if args.command == 'plan':
        value = plan()
    elif args.command == 'summarize':
        value = summarize(args.root)
    elif args.command == 'smoke':
        config = load(args.config)
        validate(config, False)
        bench = Bench(config)
        bench.prepare()
        ident = 'smoke-' + str(time.time_ns())
        stage = {'id': ident, 'trees': ['a'], 'profile': 'dev', 'jobs': 4,
                 'edits': {args.kind: ident} if args.edit else {},
                 'operation': 'build', 'checks': [args.kind]}
        trial = bench.trial(stage, 'a', bench.root / 'smoke' / ident)
        value = {'result_path': str(bench.root / 'smoke' / ident / 'result.json'),
                 'passed': trial['passed'], 'compiler_executions': trial.get('telemetry', {}).get('compiler_executions'),
                 'raw_failed_stages': int(not trial['passed']),
                 'qualification': 'targeted driver smoke only; not matrix or performance evidence'}
    else:
        config = load(args.config)
        if not args.stage and not args.go:
            parser.error('full matrix requires explicit --go after qualified implementation freeze')
        if args.stage and set(args.stage) - {stage['id'] for stage in plan()}:
            parser.error('unknown stage')
        validate(config, not args.stage)
        value = Bench(config).run(set(args.stage or []))
    print(json.dumps(value, indent=2, sort_keys=True))
    if args.command == 'plan':
        return 0
    return int(value['raw_failed_stages'] != 0)


if __name__ == '__main__':
    sys.exit(main())
