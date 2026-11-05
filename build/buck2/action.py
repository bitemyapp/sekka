#!/usr/bin/env python3
"""Execute exactly one resolved Cargo unit, never invoke Cargo to build it."""
import argparse
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys

ROOT_TOKEN = "${SEKKA_EXEC_ROOT}"
# RBE publishes a producer's output at a different path in consuming actions.
OUTPUT_TOKEN = "${SEKKA_UNIT_OUTPUT}"


def load(path):
    return json.loads(Path(path).read_text())


def relocate(value, root, output, encode=False):
    if isinstance(value, str):
        if encode:
            root, output = str(root), str(output)
            prefix = re.escape(root)
            # Cargo instructions and linker arguments can embed multiple paths.
            # Keep spaces within paths, but stop at separators or the next root.
            pattern = prefix + r"(?=/|$|[\s,:;\"'])(?:(?!" + prefix + r")[^\n\r,:;\"'])*"

            def encode_path(match):
                original = match.group()
                path = original.rstrip()
                suffix = original[len(path):]
                if path == output or path.startswith(output + "/"):
                    # Lexical `normpath` would change `link/../file` when link
                    # is a symlink. Resolve while the producer output exists.
                    referent = str(Path(path).resolve())
                    if path.endswith("/") and not referent.endswith("/"):
                        referent += "/"
                    for directory, token in [(output, OUTPUT_TOKEN), (root, ROOT_TOKEN)]:
                        if referent == directory or referent.startswith(directory + "/"):
                            return token + referent[len(directory):] + suffix
                    return referent + suffix
                # Siblings such as `unit-extra`, and unrelated input paths,
                # retain their original traversal and symlink semantics.
                if path == root or path.startswith(root + "/"):
                    return ROOT_TOKEN + path[len(root):] + suffix
                return original

            return re.sub(pattern, encode_path, value)
        return value.replace(ROOT_TOKEN, str(root)).replace(OUTPUT_TOKEN, str(output))
    if isinstance(value, list):
        return [relocate(v, root, output, encode) for v in value]
    if isinstance(value, dict):
        return {k: relocate(v, root, output, encode) for k, v in value.items()}
    return value


def native_environment(tools):
    env = {k: str(v) for k, v in tools.items() if k.isupper()}
    env.update(PATH=tools["BUILD_PATH"], CC=tools["cc"], CXX=tools["cxx"], AR=tools["ar"], RUSTC=tools["rustc"], CARGO=tools["cargo"], RUSTC_BOOTSTRAP="1", HOME="/homeless-shelter", LANG="C.UTF-8", LC_ALL="C.UTF-8")
    env.pop("BUILD_PATH", None)
    return env


def test_tmp_path(unit):
    if any(kind in {"test", "bench"} for kind in unit["target"]["kind"]):
        return Path(".buckal/test-tmp") / unit["key"]
    return None


def package_environment(unit, manifest):
    package = unit["package"]
    version = package["version"].split("-", 1)
    numbers = version[0].split(".")
    env = {"CARGO_MANIFEST_DIR": str(manifest), "CARGO_MANIFEST_PATH": str(manifest / "Cargo.toml"), "CARGO_CRATE_NAME": unit["target"]["name"].replace("-", "_"), "CARGO_PKG_VERSION": package["version"], "CARGO_PKG_VERSION_MAJOR": numbers[0], "CARGO_PKG_VERSION_MINOR": numbers[1], "CARGO_PKG_VERSION_PATCH": numbers[2], "CARGO_PKG_VERSION_PRE": version[1] if len(version) > 1 else ""}
    for key in ["name", "description", "homepage", "repository", "license", "license_file", "readme", "rust_version"]:
        env["CARGO_PKG_" + key.upper()] = package.get(key) or ""
    env["CARGO_PKG_AUTHORS"] = ":".join(package.get("authors", []))
    if package.get("links"):
        env["CARGO_MANIFEST_LINKS"] = package["links"]
    if "bin" in unit["target"]["kind"]:
        env["CARGO_BIN_NAME"] = unit["target"]["name"]
    temporary = test_tmp_path(unit)
    if temporary is not None:
        env["CARGO_TARGET_TMPDIR"] = str(Path("/proc/self/cwd") / temporary)
    return env


def profile_flags(unit):
    profile = unit["profile"]
    flags = ["-C", "opt-level=" + profile["opt_level"], "-C", "debuginfo=" + str(profile["debuginfo"]), "-C", "debug-assertions=" + str(profile["debug_assertions"]).lower(), "-C", "overflow-checks=" + str(profile["overflow_checks"]).lower(), "-C", "panic=" + profile["panic"]]
    if profile.get("codegen_units"):
        flags += ["-C", "codegen-units=" + str(profile["codegen_units"])]
    lto = unit["lto_mode"]
    if lto.startswith("run:"):
        flags += ["-C", "lto=" + lto[4:]]
    elif lto == "off":
        flags += ["-C", "lto=off"]
    elif lto == "object":
        flags += ["-C", "embed-bitcode=no"]
    elif lto == "bitcode":
        flags += ["-C", "linker-plugin-lto"]
    if profile.get("rpath"):
        flags += ["-C", "rpath"]
    if unit["package"]["name"] == "aizome-comp-core":
        flags += ["-C", "prefer-dynamic"]
    return flags


def cfg_environment(tools, flags):
    result = subprocess.run([tools["rustc"], "--print", "cfg", *flags], check=True, text=True, capture_output=True)
    values = {}
    for line in result.stdout.splitlines():
        key, sep, value = line.partition("=")
        values.setdefault("CARGO_CFG_" + key.upper(), []).append(json.loads(value) if sep else "")
    return {key: ",".join(value) for key, value in values.items()}


def script_instructions(stdout):
    result = {"env": {}, "cfg": [], "check_cfg": [], "search": [], "libs": [], "link_args": [], "metadata": {}}
    for line in stdout.splitlines():
        if line.startswith("cargo::"):
            directive = line[7:]
        elif line.startswith("cargo:"):
            directive = line[6:]
        else:
            continue
        key, sep, value = directive.partition("=")
        if not sep:
            continue
        if key == "rustc-env":
            name, _, val = value.partition("=")
            result["env"][name] = val
        elif key in {"rustc-cfg", "rustc-check-cfg", "rustc-link-search", "rustc-link-lib"}:
            result[{"rustc-cfg": "cfg", "rustc-check-cfg": "check_cfg", "rustc-link-search": "search", "rustc-link-lib": "libs"}[key]].append(value)
        elif key == "rustc-flags":
            flags = shlex.split(value)
            for i in range(0, len(flags), 2):
                if flags[i] not in {"-L", "-l"} or i + 1 == len(flags):
                    raise ValueError("Cargo rustc-flags permits only -L and -l pairs: " + value)
                result["search" if flags[i] == "-L" else "libs"].append(flags[i + 1])
        elif key.startswith("rustc-link-arg"):
            result["link_args"].append([key, value])
        elif key == "warning":
            print("build script warning: " + value, file=sys.stderr)
        elif key == "error":
            raise RuntimeError("build script error: " + value)
        elif key == "metadata":
            name, _, val = value.partition("=")
            result["metadata"][name] = val
        elif not key.startswith("rerun-if-"):
            result["metadata"][key] = value
    return result


def run_script(payload, output, env, tools, root):
    unit = payload["unit"]
    manifest = Path(payload["source"]).absolute()
    extras = payload["extra_sources"]
    if extras:
        workspace = output / "workspace"
        paths = dict(extras)
        paths[unit["package"]["path"]] = payload["source"]
        for relative, source in sorted(paths.items()):
            link = workspace / relative
            link.parent.mkdir(parents=True, exist_ok=True)
            source = Path(source).absolute()
            if any(other.startswith(relative + "/") for other in paths):
                link.mkdir(exist_ok=True)
                for child in source.iterdir():
                    (link / child.name).symlink_to(os.path.relpath(child, link), target_is_directory=child.is_dir())
            else:
                link.symlink_to(os.path.relpath(source, link.parent), target_is_directory=True)
        manifest = workspace / unit["package"]["path"]
    env.update(package_environment(unit, manifest))
    out_dir = output / "out"
    out_dir.mkdir()
    codegen = dict(unit, **unit["abi_target"]) if "abi_target" in unit else unit
    flags = profile_flags(codegen)
    if "abi_target" in unit:
        flags += ["-C", "metadata=" + codegen["key"], "-C", "extra-filename=-" + codegen["key"]]
    env.update(cfg_environment(tools, flags))
    env.update(OUT_DIR=str(out_dir), HOST=tools["target"], TARGET=unit.get("platform") or tools["target"], PROFILE="debug" if codegen["profile"]["name"] in ["dev", "test"] else "release", OPT_LEVEL=codegen["profile"]["opt_level"], DEBUG=str(codegen["profile"]["debuginfo"] not in [0, "none"]).lower(), NUM_JOBS="1", CARGO_ENCODED_RUSTFLAGS="\x1f".join(flags))
    for feature in unit["features"]:
        env["CARGO_FEATURE_" + feature.upper().replace("-", "_")] = "1"
    executable = None
    for dependency in payload["dependencies"]:
        dep = dependency["spec"]
        directory = Path(dependency["directory"]).absolute()
        if dep["mode"] == "run-custom-build":
            instructions = relocate(load(directory / "build-script.json"), root, directory)
            links = dep["package"].get("links")
            if links:
                for key, value in instructions["metadata"].items():
                    env["DEP_" + links.upper().replace("-", "_") + "_" + key.upper().replace("-", "_")] = value
        elif dep["package"]["id"] == unit["package"]["id"] and "custom-build" in dep["target"]["kind"]:
            executable = directory / dep["filename"]
    if executable is None:
        raise ValueError("Cargo unit has no build-script executable")
    completed = subprocess.run([str(executable)], cwd=manifest, env=env, text=True, stdout=subprocess.PIPE)
    (output / "stdout").write_text(relocate(completed.stdout, root, output, encode=True))
    completed.check_returncode()
    result = script_instructions(completed.stdout)
    result["out_dir"] = str(out_dir)
    (output / "build-script.json").write_text(json.dumps(relocate(result, root, output, encode=True), sort_keys=True))


def compile_unit(payload, output, env, tools, root):
    unit = payload["unit"]
    target = unit["target"]
    manifest = Path(payload["source"]).absolute()
    env.update(package_environment(unit, manifest))
    flags = profile_flags(unit)
    args = [tools["rustc"], str(manifest / target["src_path"]), "--crate-name", target["name"].replace("-", "_"), "--edition=" + target["edition"], "--emit=link", "-C", "metadata=" + unit["key"], "-C", "extra-filename=-" + unit["key"], "--out-dir", str(output), "--remap-path-prefix", str(root) + "=.", "-C", "linker=" + tools["cc"], *flags]
    args += ["--remap-path-prefix", "/proc/self/cwd=."]
    if unit["mode"] == "test":
        args.append("--test")
    else:
        for kind in target["crate_types"]:
            args += ["--crate-type", "rlib" if kind == "lib" else kind]
    if "proc-macro" in target["crate_types"]:
        args += ["--extern", "proc_macro"]
    if unit["profile"]["name"] == "dev":
        args += ["-C", "link-arg=-B" + str(Path(tools["mold"]).parent), "-C", "link-arg=-fuse-ld=mold"]
    for flag in shlex.split(tools.get("NIX_LDFLAGS", "")):
        if flag.startswith("-L"):
            args += ["-L", flag[2:]]
    for feature in unit["features"]:
        args += ["--cfg", 'feature="' + feature + '"']
    args += ["--check-cfg", "cfg(docsrs,test)", "--check-cfg", 'cfg(feature, values(' + ",".join(json.dumps(f) for f in unit["package"]["features"]) + '))']
    for directory in payload["closure"]:
        directory = Path(directory).absolute()
        args += ["-L", "dependency=" + str(directory)]
        instructions = directory / "build-script.json"
        if instructions.exists():
            for search in relocate(load(instructions), root, directory)["search"]:
                args += ["-L", search]
    for dependency, edge in zip(payload["dependencies"], unit["dependencies"]):
        dep = dependency["spec"]
        directory = Path(dependency["directory"]).absolute()
        if dep["mode"] == "run-custom-build":
            if dep["package"]["id"] != unit["package"]["id"]:
                continue
            instructions = relocate(load(directory / "build-script.json"), root, directory)
            env.update(instructions["env"])
            env["OUT_DIR"] = instructions["out_dir"]
            for value in instructions["cfg"]:
                args += ["--cfg", value]
            for value in instructions["check_cfg"]:
                args += ["--check-cfg", value]
            if "lib" in target["kind"] or "dylib" in target["kind"] or not unit["package"]["has_lib"]:
                for value in instructions["libs"]:
                    args += ["-l", value]
            for key, value in instructions["link_args"]:
                if key == "rustc-link-arg-bin":
                    name, _, value = value.partition("=")
                    if target["name"] != name or "bin" not in target["kind"]:
                        continue
                elif key == "rustc-link-arg-bins" and "bin" not in target["kind"]:
                    continue
                elif key == "rustc-link-arg-tests" and unit["mode"] != "test":
                    continue
                elif key == "rustc-link-arg-cdylib" and "cdylib" not in target["crate_types"]:
                    continue
                elif key not in {"rustc-link-arg", "rustc-link-arg-bin", "rustc-link-arg-bins", "rustc-link-arg-tests", "rustc-link-arg-cdylib"}:
                    continue
                args += ["-C", "link-arg=" + value]
        elif "bin" in dep["target"]["kind"]:
            env["CARGO_BIN_EXE_" + dep["target"]["name"]] = str(directory / dep["filename"])
        else:
            args += ["--extern", edge["extern_crate_name"] + "=" + str(directory / dep["filename"])]
    if unit["package"]["external"]:
        args += ["--cap-lints", "allow"]
    # Linux's process-relative absolute path works both for include!(env!(...))
    # during rustc and env!("CARGO_MANIFEST_DIR") in a cached test on a new tree.
    # Unlike the checkout's absolute path, it is identical on every executor.
    env = {key: value.replace(str(root), "/proc/self/cwd") for key, value in env.items()}
    subprocess.run(args, env=env, check=True)
    if not (output / unit["filename"]).exists():
        raise RuntimeError("rustc did not produce expected output " + unit["filename"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--environment", required=True)
    parser.add_argument("--spec", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    tools = load(args.environment)
    root = Path.cwd()
    env = native_environment(tools)
    payload = load(args.spec)
    output = Path(args.output).absolute()
    output.mkdir(parents=True, exist_ok=True)
    library_paths = [str(Path(p).absolute()) for p in payload["closure"]]
    for directory in payload["closure"]:
        instructions = Path(directory) / "build-script.json"
        if instructions.exists():
            for search in relocate(load(instructions), root, Path(directory).absolute())["search"]:
                kind, separator, path = search.partition("=")
                path = path if separator and kind in {"native", "dependency", "all", "crate", "framework"} else search
                if path not in library_paths:
                    library_paths.append(path)
    env["LD_LIBRARY_PATH"] = ":".join([*library_paths, tools["rustlib"], tools.get("LIBRARY_PATH", "")])
    env["NIX_CFLAGS_COMPILE"] = env.get("NIX_CFLAGS_COMPILE", "") + " -ffile-prefix-map=" + str(root) + "=. -fdebug-prefix-map=" + str(root) + "=."
    env["ZERO_AR_DATE"] = "1"
    if payload["unit"]["mode"] == "run-custom-build":
        run_script(payload, output, env, tools, root)
    else:
        compile_unit(payload, output, env, tools, root)
    info = {"filename": payload["unit"]["filename"], "library_paths": library_paths}
    temporary = test_tmp_path(payload["unit"])
    if temporary is not None:
        info["test_tmp"] = str(temporary)
    (output / "unit.json").write_text(json.dumps(relocate(info, root, output, encode=True), sort_keys=True))


if __name__ == "__main__":
    main()
