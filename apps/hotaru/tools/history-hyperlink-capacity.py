#!/usr/bin/env python3
"""Reproduce the pinned reference's 1,663-link page limit, keeping raw evidence.

Requires the memory audit's build.json, which identifies both adapter binaries.
This diagnostic deliberately does not benchmark unequal retained content.
"""
import argparse
import fcntl
import hashlib
import json
from pathlib import Path
import subprocess


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-root', type=Path, required=True)
    parser.add_argument('--build', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    build = json.loads(args.build.read_text())
    source = Path(__file__).with_suffix('.c')
    executable = args.output/'hyperlink-capacity-probe'
    reference = args.reference_root
    command = ['cc', '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror',
               '-I'+str(reference/'include'), str(source), '-L'+str(reference/'zig-out/lib'),
               '-Wl,-rpath,'+str(reference/'zig-out/lib'), '-lghostty-vt', '-o', str(executable)]
    report = {'schema': 1, 'reference_commit': subprocess.check_output(['git', '-C', str(reference), 'rev-parse', 'HEAD'], text=True).strip(),
              'reference_library_sha256': sha(reference/'zig-out/lib/libghostty-vt.so'),
              'probe_source_sha256': sha(source), 'driver_sha256': sha(__file__),
              'build_metadata_sha256': sha(args.build), 'build_command': command,
              'cases': [], 'note': 'Reference omits valid links after its page-local capacity is exhausted. Hotaru intentionally retains these links. No speed or memory ratio is inferred from unequal contents.'}
    with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        subprocess.run(command, check=True)
        for count in [1663, 1664, 1665]:
            path = args.output/f'unique-links-{count}.vt'
            path.write_bytes(b''.join(b'\x1b]8;;https://x/'+str(i).encode()+b'\x1b\\x\x1b]8;;\x1b\\' for i in range(count)))
            case = {'count': count, 'input_sha256': sha(path), 'input_bytes': path.stat().st_size,
                    'reference_cells': json.loads(subprocess.check_output([str(executable), str(path), str(count)], text=True)), 'adapters': {}}
            for name in ['reference', 'candidate']:
                assert sha(build[name]) == build[name+'_sha256']
                run = [build[name], str(path), '120', '24', '10000', '50000000', '0']
                if name == 'candidate':
                    run += ['none', '0']
                case['adapters'][name] = {'command': run, 'output': json.loads(subprocess.check_output(run, text=True))}
            report['cases'].append(case)
        report['probe_binary_sha256'] = sha(executable)
    (args.output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps([case['reference_cells'] for case in report['cases']], indent=2))


if __name__ == '__main__':
    main()
