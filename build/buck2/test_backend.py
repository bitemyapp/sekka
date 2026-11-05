#!/usr/bin/env python3
"""Real pinned-rustc regression: Cargo resolves; individual actions compile/run."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from generate import outside_data, package_records, units_from_graph
from action import OUTPUT_TOKEN, ROOT_TOKEN, relocate


class CompilerContract(unittest.TestCase):
    def checked(self, command, *, cwd, env):
        result = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, "Command: " + repr(command) + "\n" + result.stdout + result.stderr)
        return result.stdout

    def test_output_relocation_respects_path_ownership(self):
        with tempfile.TemporaryDirectory(prefix="sekka-buck-relocation-") as temporary:
            producer = Path(temporary) / "producer"
            consumer = Path(temporary) / "consumer"
            output = producer / "output_artifacts/unit"
            materialized = consumer / "input_artifacts/hash/unit"
            (output / "nested/subdir").mkdir(parents=True)
            materialized.parent.mkdir(parents=True)
            (output / "generated.h").write_text("generated")
            (output / "file with spaces").write_text("spaced")
            (output / "nested/header.h").write_text("symlink referent")
            (output / "link").symlink_to("nested/subdir", target_is_directory=True)
            for root in [producer, consumer]:
                (root / "assets").write_text("root-owned")
                sibling = root / "output_artifacts/unit-extra"
                sibling.mkdir(parents=True)
                (sibling / "data").write_text("sibling-owned")
                (root / "source/nested/subdir").mkdir(parents=True)
                (root / "source/nested/header.h").write_text("source referent")
                (root / "source/link").symlink_to("nested/subdir", target_is_directory=True)
            cases = [
                (str(output / "generated.h"), OUTPUT_TOKEN + "/generated.h", materialized / "generated.h"),
                (str(output / "nested/../generated.h"), OUTPUT_TOKEN + "/generated.h", materialized / "generated.h"),
                (str(output / "file with spaces"), OUTPUT_TOKEN + "/file with spaces", materialized / "file with spaces"),
                (str(output / "../../assets"), ROOT_TOKEN + "/assets", consumer / "assets"),
                (str(output) + "-extra/data", ROOT_TOKEN + "/output_artifacts/unit-extra/data", consumer / "output_artifacts/unit-extra/data"),
                (str(output / "link/../header.h"), OUTPUT_TOKEN + "/nested/header.h", materialized / "nested/header.h"),
                (str(producer / "source/link/../header.h"), ROOT_TOKEN + "/source/link/../header.h", consumer / "source/link/../header.h"),
            ]
            contents = {original: Path(original).read_text() for original, _, _ in cases}
            encoded_paths = [relocate(original, producer, output, encode=True) for original, _, _ in cases]
            # Exercise the actual metadata shapes, not only standalone paths.
            metadata = {
                "search": ["native=" + cases[3][0]],
                "paths": ":".join(case[0] for case in cases),
                "stdout": "cargo:root=" + cases[0][0] + "\ncargo:assets=" + cases[3][0] + "\n",
            }
            encoded = relocate(metadata, producer, output, encode=True)
            # Encoding happens in the producer; decoding happens only after its
            # complete output, including relative symlinks, moves to a new path.
            shutil.move(output, materialized)
            for actual, (original, expected_encoding, expected_path) in zip(encoded_paths, cases):
                with self.subTest(path=original):
                    self.assertEqual(actual, expected_encoding)
                    decoded_path = relocate(actual, consumer, materialized)
                    self.assertEqual(decoded_path, str(expected_path))
                    self.assertEqual(Path(decoded_path).read_text(), contents[original])
            self.assertEqual((materialized / "link/../header.h").read_text(), "symlink referent")
            decoded = relocate(encoded, consumer, materialized)
            self.assertEqual(decoded["search"], ["native=" + str(consumer / "assets")])
            self.assertEqual(decoded["paths"], ":".join(str(case[2]) for case in cases))
            self.assertEqual(decoded["stdout"], "cargo:root=" + str(materialized / "generated.h") + "\ncargo:assets=" + str(consumer / "assets") + "\n")

    def test_sibling_resources_are_action_inputs(self):
        environment = Path(os.environ["SEKKA_BUCK2_ENV"]).resolve()
        tools = json.loads(environment.read_text())
        engine = os.environ["SEKKA_BUCK2_BIN"]
        with tempfile.TemporaryDirectory(prefix="sekka-buck-source-inputs-") as temporary:
            root = Path(temporary) / "workspace"
            home = Path(temporary) / "home"
            home.mkdir()
            env = dict(os.environ, HOME=str(home), CARGO_HOME=str(home / "cargo"),
                       RUSTC=tools["rustc"], RUSTC_BOOTSTRAP="1",
                       BUCK2_TEST_SKIP_DEFAULT_EXTERNAL_CONFIG="true")
            for key in ["BUCK2_EXTRA_EXTERNAL_CONFIG", "BUCK2_LOCAL_CONTENT_STORE", "BUCK2_DAEMON_DIR"]:
                env.pop(key, None)
            initial = "<svg>declared resource</svg>\n"
            changed = "<svg>changed resource</svg>\n"
            files = {
                "Cargo.toml": '[workspace]\nresolver="3"\nmembers=["library"]\n',
                "library/Cargo.toml": '[package]\nname="resource-fixture"\nversion="0.1.0"\nedition="2024"\n',
                "library/src/main.rs": '''macro_rules! icon {
    ($name:literal) => { include_str!(concat!("../../icons/", $name, ".svg")) };
}
fn main() { print!("{}", icon!("sample")); }
''',
                "icons/sample.svg": initial,
                "unrelated/secret.txt": "not an input\n",
                ".buckconfig": '''[cells]
root = .
prelude = prelude
none = none
toolchains = toolchains
[cell_aliases]
config = prelude
ovr_config = prelude
fbcode = none
fbsource = none
fbcode_macros = none
buck = none
[external_cells]
prelude = bundled
[parser]
target_platform_detector_spec = target:root//...->prelude//platforms:default target:prelude//...->prelude//platforms:default
[build]
execution_platforms = root//build/buck2:execution
''',
                "build/buck2/BUCK": '''load(":platforms.bzl", "execution")
execution(name = "execution")
export_file(name = "action.py", visibility = ["PUBLIC"])
export_file(name = "run.py", visibility = ["PUBLIC"])
''',
            }
            for relative, text in files.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
            for name in ["units.bzl", "platforms.bzl", "action.py", "run.py"]:
                shutil.copyfile(Path(__file__).with_name(name), root / "build/buck2" / name)
            metadata = json.loads(self.checked([tools["cargo"], "metadata", "--format-version", "1", "--offline"], cwd=root, env=env))
            graph = json.loads(self.checked([tools["cargo"], "build", "--locked", "--offline", "--unit-graph", "-Z", "unstable-options"], cwd=root, env=env))
            units, _ = units_from_graph(graph, package_records(metadata, root), metadata)
            self.assertEqual(len(graph["roots"]), 1)
            unit = units[graph["roots"][0]]
            resources = outside_data(root, "library")
            self.assertEqual(resources, ["icons/sample.svg"])
            inputs = {path.relative_to(root).as_posix(): path.relative_to(root).as_posix()
                      for path in (root / "library").rglob("*") if path.is_file()}
            inputs.update({path: path for path in resources})
            (root / "BUCK").write_text(
                'load("//build/buck2:units.bzl", "cargo_source", "cargo_unit")\n'
                'cargo_source(name = "source", prefix = "library", files = ' + json.dumps(inputs) + ')\n'
                'cargo_unit(name = "unit", source = ":source", spec = ' + json.dumps(json.dumps(unit)) + ')\n'
            )
            trace_file = Path(temporary) / "build-id"
            try:
                for relative, contents, expected, executions in [
                    (None, None, initial, 1),
                    ("icons/sample.svg", changed, changed, 1),
                    ("unrelated/secret.txt", "still not an input\n", changed, 0),
                ]:
                    if relative is not None:
                        (root / relative).write_text(contents)
                    outputs = json.loads(self.checked(
                        [engine, "build", "//:unit", "--show-full-json-output", "--write-build-id", str(trace_file), "--config-file", str(environment.with_name("buckconfig"))],
                        cwd=root, env=env,
                    ))
                    executable = next(iter(outputs.values()))
                    self.assertEqual(self.checked([executable], cwd=root, env=env), expected)
                    # Local symlinks can hide a missing sibling input. A real
                    # resource edit must invalidate the action, not reuse stale bytes.
                    # Select this exact invocation, not filesystem-dependent
                    # "latest" log ordering for several builds in one second.
                    trace_id = trace_file.read_text().strip()
                    commands = self.checked([engine, "log", "what-ran", "--trace-id", trace_id, "--format", "json"], cwd=root, env=env)
                    commands = [json.loads(line) for line in commands.splitlines()]
                    self.assertEqual(len(commands), executions, (relative, trace_id, commands))
                    for command in commands:
                        self.assertEqual(command["reproducer"]["executor"], "Local")
            finally:
                self.checked([engine, "kill"], cwd=root, env=env)

    def test_real_compiler_runtime_and_test_contracts(self):
        environment = Path(os.environ["SEKKA_BUCK2_ENV"]).resolve()
        tools = json.loads(environment.read_text())
        action = Path(__file__).with_name("action.py").resolve()
        runner = Path(__file__).with_name("run.py").resolve()
        runtime = Path(os.environ["SEKKA_BUCK2_RUNTIME"]).resolve()
        with tempfile.TemporaryDirectory(prefix="sekka-buck-contract-") as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            staged_runner = root / "runtime-helper" / "run.py"
            staged_runner.parent.mkdir()
            shutil.copyfile(runner, staged_runner)
            runner = staged_runner
            temporary_contract = '''#![deny(unexpected_cfgs)]
#[cfg(docsrs)]
compile_error!("docsrs must be declared, not enabled");
#[test]
fn target_tmp_is_worktree_private() {
    let directory = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    assert_eq!(directory, std::path::PathBuf::from(std::env::var("CARGO_TARGET_TMPDIR").unwrap()));
    assert!(directory.is_dir());
    let marker = directory.join("worktree");
    assert!(!marker.exists(), "a different worktree must not seed this directory");
    std::fs::write(&marker, std::env::current_dir().unwrap().to_str().unwrap()).unwrap();
    let child = std::process::Command::new(__PYTHON__)
        .arg("-c")
        .arg("import subprocess,sys; subprocess.run([sys.executable, '-c', 'import os,sys; from pathlib import Path; p=Path(sys.argv[1]); assert p.read_text()==os.getcwd(); p.with_name(\\\"child\\\").write_text(\\\"ok\\\")', sys.argv[1]], check=True, close_fds=True)")
        .arg(&marker)
        .status().unwrap();
    assert!(child.success());
    assert_eq!(std::fs::read_to_string(directory.join("child")).unwrap(), "ok");
}
'''.replace("__PYTHON__", json.dumps(sys.executable))
            files = {
                "Cargo.toml": '''[workspace]
resolver = "3"
members = ["shared", "macros", "native", "core", "app"]
[profile.release-lto]
inherits = "release"
lto = "thin"
codegen-units = 1
[profile.release-lto.build-override]
opt-level = 3
codegen-units = 1
[profile.release-lto.package.aizome-comp-core]
overflow-checks = true
''',
                "shared/Cargo.toml": '[package]\nname="contract-shared"\nversion="0.1.0"\nedition="2024"\n',
                "shared/src/lib.rs": 'pub fn value() -> i32 { 7 }\n',
                "macros/Cargo.toml": '''[package]
name="contract-macros"
version="0.1.0"
edition="2024"
[lib]
proc-macro=true
[dependencies]
contract-shared={path="../shared"}
''',
                "macros/src/lib.rs": '''use proc_macro::TokenStream;
#[proc_macro]
pub fn answer(_: TokenStream) -> TokenStream {
    contract_shared::value().to_string().parse().unwrap()
}
''',
                "native/Cargo.toml": '[package]\nname="contract-native"\nversion="0.1.0"\nedition="2024"\nlinks="contract_native"\n',
                "native/native.c": 'int native_value(void) { return 11; }\n',
                "native/build.rs": '''use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:warning=compiler contract warning-path regression");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let status = Command::new(env::var_os("CC").unwrap())
        .args(["-shared", "-fPIC", "native.c", "-o"])
        .arg(out.join("libcontract_native.so")).status().unwrap();
    assert!(status.success());
    fs::write(out.join("generated.rs"), "const GENERATED: i32 = 3;\\n").unwrap();
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib=contract_native");
}
''',
                "native/src/lib.rs": '''include!(concat!(env!("OUT_DIR"), "/generated.rs"));
unsafe extern "C" { fn native_value() -> i32; }
pub fn value() -> i32 { unsafe { native_value() + GENERATED } }
''',
                "core/Cargo.toml": '''[package]
name="aizome-comp-core"
version="0.1.0"
edition="2024"
[lib]
crate-type=["dylib"]
[dependencies]
contract-shared={path="../shared"}
''',
                "core/build.rs": '''fn main() {
    let flags = std::env::var("CARGO_ENCODED_RUSTFLAGS").unwrap().replace('\\u{1f}', " ");
    println!("cargo:rustc-env=PROBE_RUSTFLAGS={flags}");
}
''',
                "core/src/lib.rs": '''#[unsafe(no_mangle)]
pub extern "C" fn probe_value() -> i32 { contract_shared::value() }
#[unsafe(no_mangle)]
pub extern "C" fn probe_flags() -> *const std::ffi::c_char {
    concat!(env!("PROBE_RUSTFLAGS"), "\\0").as_ptr().cast()
}
''',
                "app/Cargo.toml": '''[package]
name="contract-app"
version="0.1.0"
edition="2024"
[dependencies]
contract-macros={path="../macros"}
contract-native={path="../native"}
''',
                "app/src/main.rs": '''fn main() {
    assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), ["--runtime", "user value", "--environment", "app-env", "--", "literal"]);
    assert_eq!(contract_macros::answer!() + contract_native::value(), 21);
    println!("real LTO, proc-macro, generated Rust and native dylib: 21");
}
''',
                "app/tests/temporary.rs": temporary_contract,
                "app/benches/temporary.rs": temporary_contract,
            }
            for name, content in files.items():
                path = workspace / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content)
            cargo_env = dict(os.environ, RUSTC=tools["rustc"], RUSTC_BOOTSTRAP="1", CARGO_HOME=str(root / "cargo-home"), CARGO_TARGET_DIR=str(root / "cargo-query"))
            for name in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"]:
                cargo_env.pop(name, None)
            metadata = json.loads(self.checked([tools["cargo"], "metadata", "--format-version", "1", "--offline"], cwd=workspace, env=cargo_env))
            graph = json.loads(self.checked([tools["cargo"], "build", "--workspace", "--profile", "release-lto", "--locked", "--offline", "--unit-graph", "-Z", "unstable-options"], cwd=workspace, env=cargo_env))
            units, _ = units_from_graph(graph, package_records(metadata, root), metadata)
            completed = {}
            closures = {}
            built = {}
            (root / "outputs").mkdir()

            def build(index):
                if index in completed:
                    return
                unit = units[index]
                if unit["key"] in built:
                    completed[index], closures[index] = built[unit["key"]]
                    return
                dependencies = graph["units"][index]["dependencies"]
                closure = set()
                for dependency in dependencies:
                    dep = dependency["index"]
                    build(dep)
                    closure.add(completed[dep])
                    closure.update(closures[dep])
                output = "outputs/" + unit["key"]
                payload = {
                    "unit": unit,
                    "source": unit["package"]["path"],
                    "dependencies": [{"spec": units[dep["index"]], "directory": completed[dep["index"]]} for dep in dependencies],
                    "closure": sorted(closure),
                    "extra_sources": {},
                }
                specification = root / (unit["key"] + ".json")
                specification.write_text(json.dumps(payload))
                producer_output = "producer-outputs/" + unit["key"]
                self.checked([sys.executable, str(action), "--environment", str(environment), "--spec", str(specification), "--output", producer_output], cwd=root, env=cargo_env)
                # RBE publishes an output at a different path in the next action.
                shutil.move(root / producer_output, root / output)
                for name in ["build-script.json", "unit.json", "stdout"]:
                    metadata_file = root / output / name
                    if metadata_file.exists():
                        self.assertNotIn("producer-outputs", metadata_file.read_text())
                completed[index] = output
                closures[index] = closure
                built[unit["key"]] = output, closure

            for index in graph["roots"]:
                build(index)
            app = next(i for i, unit in enumerate(units) if unit["package"]["name"] == "contract-app" and unit["mode"] == "build")
            self.assertEqual(units[app]["lto_mode"], "run:thin")
            result = self.checked([sys.executable, str(runner), "--action-module", str(action), "--environment", str(environment), "--runtime", str(runtime), "--run", completed[app], "--", "--runtime", "user value", "--environment", "app-env", "--", "literal"], cwd=root, env=cargo_env)
            self.assertIn("native dylib: 21", result)
            core = next(i for i, unit in enumerate(units) if unit["package"]["name"] == "aizome-comp-core" and unit["mode"] == "build" and "dylib" in unit["target"]["crate_types"])
            library = root / completed[core] / units[core]["filename"]
            probe = "import ctypes,json,sys; lib=ctypes.CDLL(sys.argv[1]); lib.probe_flags.restype=ctypes.c_char_p; print(json.dumps([lib.probe_value(),lib.probe_flags().decode()]))"
            value, flags = json.loads(self.checked([sys.executable, "-c", probe, str(library)], cwd=root, env=dict(cargo_env, LD_LIBRARY_PATH=tools["rustlib"])))
            self.assertEqual(value, 7)
            self.assertIn("overflow-checks=true", flags)
            self.assertIn("metadata=" + units[core]["key"], flags)
            self.assertTrue(any(unit["lto_mode"] == "object-bitcode" for unit in units if unit["package"]["name"] == "contract-shared"))
            for kind, selection in [("test", "--test"), ("bench", "--bench")]:
                graph = json.loads(self.checked([tools["cargo"], "test", "-p", "contract-app", selection, "temporary", "--no-run", "--profile", "release-lto", "--locked", "--offline", "--unit-graph", "-Z", "unstable-options"], cwd=workspace, env=cargo_env))
                units, _ = units_from_graph(graph, package_records(metadata, root), metadata)
                completed = {}
                closures = {}
                for index in graph["roots"]:
                    build(index)
                index = next(i for i, unit in enumerate(units) if kind in unit["target"]["kind"])
                info = json.loads((root / completed[index] / "unit.json").read_text())
                self.assertEqual(info["test_tmp"], ".buckal/test-tmp/" + units[index]["key"])
                restored = root / ("restored-" + kind)
                shutil.copytree(workspace, restored / "workspace")
                shutil.copytree(root / "outputs", restored / "outputs")
                for execution_root in [root, restored]:
                    result = self.checked([sys.executable, str(runner), "--action-module", str(action), "--environment", str(environment), "--runtime", str(runtime), "--run", completed[index], "--test-run", "--"], cwd=execution_root, env=cargo_env)
                    self.assertIn("1 passed", result)
                    directory = execution_root / info["test_tmp"]
                    self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
                    self.assertEqual((directory / "worktree").read_text(), str(execution_root))
                self.assertEqual((root / info["test_tmp"] / "worktree").read_text(), str(root))
            self.assertFalse(list((root / "cargo-query").glob("**/*.rlib")), "Cargo must only query graphs, not compile")


if __name__ == "__main__":
    unittest.main()
