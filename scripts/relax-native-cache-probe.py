#!/usr/bin/env python3
"""Trace Cargo preparation for a generated-source-only native build script."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('runner', type=Path)
parser.add_argument('std', type=Path)
parser.add_argument('dest', type=Path)
parser.add_argument('--warm-checks', type=int, choices=(0, 1), default=0)
args = parser.parse_args()
runner, std, dest = args.runner, args.std, args.dest
runner, std, dest = runner.resolve(), std.resolve(), dest.resolve()
dest.mkdir()
fixture = dest / 'fixture'
fixture.mkdir()
(fixture / 'src').mkdir()
(fixture / 'Cargo.toml').write_text('[workspace]\n[package]\nname="relax-cfg-cache-probe"\nversion="0.1.0"\nedition="2021"\nlinks="cfg_cache_probe"\n')
(fixture / 'build.rs').write_text('fn main() { let p=std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()); std::fs::write(p.join("generated.rs"), "pub const GENERATED: u32 = 17;").unwrap(); println!("cargo:rustc-cfg=cfg_cache_probe"); }\n')
(fixture / 'src/lib.rs').write_text('include!(concat!(env!("OUT_DIR"), "/generated.rs")); #[test] fn generated_value() { assert!(cfg!(cfg_cache_probe)); assert_eq!(GENERATED, 17); }\n')
binary = dest / 'bin'
binary.mkdir()
cargo = shutil.which('cargo')
trace = dest / 'cargo.jsonl'
wrapper = binary / 'cargo'
wrapper.write_text(f'#!{sys.executable}\nimport os,sys,json\nwith open({str(trace)!r},"a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\nos.execv({cargo!r}, [{cargo!r}, *sys.argv[1:]])\n')
wrapper.chmod(0o755)
env = dict(os.environ, PATH=str(binary)+os.pathsep+os.environ['PATH'], MAKEPAD_RUN_STD=str(std), MAKEPAD_RUN_CACHE_DIR=str(dest/'cache'), CARGO_TARGET_DIR=str(dest/'target'), MAKEPAD_CPU_CORES='4', CARGO_BUILD_JOBS='4')
results=[]
signal.signal(signal.SIGTERM, lambda number, frame: sys.exit(128 + number))
for stage in ['cold', 'warm', 'missing-output']:
    if stage == 'missing-output':
        for path in (dest/'cache').rglob('messages.jsonl'):
            for line in path.read_text().splitlines():
                out = Path(json.loads(line)['out_dir']).resolve()
                assert out.is_relative_to(dest) and out != dest, out
                if out.exists(): shutil.rmtree(out)
    before = len(trace.read_text().splitlines()) if trace.exists() else 0
    code, error = None, None
    with (dest/f'{stage}.log').open('w') as log:
        try:
            proc = subprocess.Popen([str(runner), 'test', '--no-live', '--allow-c', 'relax-cfg-cache-probe', '--lib'], cwd=fixture,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        except OSError as exc:
            error = str(exc)
        else:
            try:
                code = proc.wait(timeout=180)
            except subprocess.TimeoutExpired:
                error = 'command timed out'
            finally:
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                proc.wait()
    calls=[json.loads(x) for x in trace.read_text().splitlines()[before:]] if trace.exists() else []
    checks=sum('check' in c for c in calls)
    results.append(dict(stage=stage,exit_code=code,cargo_checks=checks,cargo_calls=calls,error=error))
    (dest/'results.json').write_text(json.dumps(results,indent=2)+'\n')
    print(stage, code, checks, flush=True)
    if error:
        sys.exit(1)
expected_checks = [1, args.warm_checks, 1]
for row, expected in zip(results, expected_checks):
    # Cargo itself does not regenerate a deleted OUT_DIR automatically here.
    # The last stage must fail explicitly, after trying Cargo again.
    status = row['exit_code'] == 0 if row['stage'] != 'missing-output' else row['exit_code'] in (1, 101)
    row['passed'] = status and row['cargo_checks'] == expected
(dest/'results.json').write_text(json.dumps(results,indent=2)+'\n')
sys.exit(int(any(not x['passed'] for x in results)))
