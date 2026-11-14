#!/usr/bin/env python3
"""Execute exactly one resolved Cargo unit, never invoke Cargo to build it."""
import argparse
import errno
import fcntl
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

ROOT_TOKEN = "${SEKKA_EXEC_ROOT}"
# RBE publishes a producer's output at a different path in consuming actions.
OUTPUT_TOKEN = "${SEKKA_UNIT_OUTPUT}"
# Match owned paths at token boundaries, including joined native search options.
EXECUTION_PATH_PREFIX = r"(?<![A-Za-z0-9_./-])(-[LIBF])?"
EXECUTION_PATH_END = r"(?=/|$|[\s,:;\"'])"


def load(path):
    return json.loads(Path(path).read_text())


def relocate(value, root, output, encode=False):
    if isinstance(value, str):
        if encode:
            root, output = str(root), str(output)
            prefix = re.escape(root)
            # Cargo instructions and linker arguments can embed multiple paths.
            # Keep spaces within paths, but stop at separators or the next root.
            start = EXECUTION_PATH_PREFIX + prefix + EXECUTION_PATH_END
            pattern = start + r"(?:(?!" + start + r")[^\n\r,:;\"'])*"

            def encode_path(match):
                option = match[1] or ""
                original = match.group()[len(option):]
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
                            return option + token + referent[len(directory):] + suffix
                    # A checkout may itself sit below an alias (macOS /var
                    # points at /private/var). Compare resolved bases only
                    # after the ordinary fast path; preserve lexical sibling
                    # paths and their symlink traversal below.
                    for directory, token in [(output, OUTPUT_TOKEN), (root, ROOT_TOKEN)]:
                        directory = str(Path(directory).resolve())
                        if referent == directory or referent.startswith(directory + "/"):
                            return option + token + referent[len(directory):] + suffix
                    return option + referent + suffix
                # Siblings such as `unit-extra`, and unrelated input paths,
                # retain their original traversal and symlink semantics.
                if path == root or path.startswith(root + "/"):
                    return option + ROOT_TOKEN + path[len(root):] + suffix
                return option + original

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


def write_dependencies(raw, destination, root):
    # Rustc repeats its source list for each output, then emits empty source
    # rules and env-dep comments. Only the first rule contains the input list.
    rule = raw.read_text().split("\n\n", 1)[0].replace("\\\n", "")
    _, separator, dependencies = rule.partition(": ")
    if not separator:
        raise RuntimeError("rustc produced an invalid dependency rule")
    paths = set()
    # Rustc escapes spaces only: quotes, '#', '$', and backslashes before
    # ordinary characters are literal filename characters, not shell syntax.
    for dependency in re.split(r"(?<!\\) ", dependencies.rstrip("\r\n")):
        if not dependency:
            continue
        dependency = dependency.replace("\\ ", " ")
        if dependency.startswith("/proc/self/cwd/"):
            dependency = str(root / dependency.removeprefix("/proc/self/cwd/"))
        path = Path(dependency)
        if path.is_absolute():
            try:
                path = path.relative_to(root)
            except ValueError:
                # Immutable Nix inputs are represented by their store paths,
                # not by project artifacts eligible for depfile filtering.
                if path.is_relative_to("/nix/store"):
                    continue
                raise RuntimeError("rustc dependency lies outside the execution root: " + str(path))
        if "\n" in str(path) or "\r" in str(path):
            raise RuntimeError("rustc dependency contains an unsupported newline")
        normalized = Path(os.path.normpath(path))
        if normalized.parts and normalized.parts[0] == "..":
            raise RuntimeError("rustc dependency lies outside the execution root: " + str(path))
        if not (root / path).is_file():
            raise RuntimeError("rustc dependency is not an existing file: " + str(path))
        if ".." in path.parts and (root / normalized).resolve() != (root / path).resolve():
            raise RuntimeError("rustc dependency traverses a directory symlink with '..': " + str(path))
        paths.add(normalized.as_posix())
    Path(destination).write_text("".join(path + "\n" for path in sorted(paths)))


def compile_unit(payload, output, env, tools, root, dep_file, incremental_directory):
    unit = payload["unit"]
    target = unit["target"]
    manifest = Path(payload["source"]).absolute()
    env.update(package_environment(unit, manifest))
    flags = profile_flags(unit)
    if incremental_directory is not None:
        flags += ["-C", "incremental=" + str(incremental_directory)]
    raw_dep_file = output / "rustc.d"
    emit = "link,dep-info=" + str(raw_dep_file) if dep_file is not None else "link"
    args = [tools["rustc"], str(manifest / target["src_path"]), "--crate-name", target["name"].replace("-", "_"), "--edition=" + target["edition"], "--emit=" + emit, "-C", "metadata=" + unit["key"], "-C", "extra-filename=-" + unit["key"], "--out-dir", str(output), "-C", "linker=" + tools["cc"], *flags]
    # Rustc tracks the actual working directory independently of argv paths.
    args += ["--remap-path-prefix", "/proc/self/cwd=.", "-Z", "remap-cwd-prefix=."]
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
    # Joined search-path options still contain an owned absolute path. Keep
    # their option prefix without matching a root embedded in an external path.
    execution_root = re.compile(EXECUTION_PATH_PREFIX + re.escape(str(root)) + EXECUTION_PATH_END)
    args = [execution_root.sub(r"\1/proc/self/cwd", value) for value in args]
    env = {key: execution_root.sub(r"\1/proc/self/cwd", value) for key, value in env.items()}
    subprocess.run(args, env=env, check=True)
    if dep_file is not None:
        write_dependencies(raw_dep_file, dep_file, root)
        raw_dep_file.unlink()
    if not (output / unit["filename"]).exists():
        raise RuntimeError("rustc did not produce expected output " + unit["filename"])


def own_readonly_incremental_files(directory, env):
    # Imported state may share read-only inodes with the CAS. Rustc requires
    # writable locks and reused objects; detach before changing any mode.
    for path in directory.rglob("*"):
        if not path.is_file():
            continue
        status = path.stat()
        if status.st_mode & 0o200:
            continue
        descriptor, temporary = tempfile.mkstemp(prefix=".owned-", dir=path.parent)
        try:
            cloned = False
            with os.fdopen(descriptor, "wb") as destination, path.open("rb") as source:
                try:
                    fcntl.ioctl(destination.fileno(), 0x40049409, source.fileno())  # Linux FICLONE
                    cloned = True
                except OSError as error:
                    if error.errno not in {errno.EXDEV, errno.EOPNOTSUPP, errno.ENOTTY, errno.EINVAL}:
                        raise
            if not cloned:
                if status.st_blocks * 512 < status.st_size:
                    subprocess.run(["cp", "--reflink=auto", "--sparse=always", "--", str(path), temporary], env=env, check=True)
                else:
                    # The pinned Python uses the platform's native fast-copy path.
                    shutil.copyfile(path, temporary)
            os.chmod(temporary, (status.st_mode & 0o777) | 0o200)
            os.replace(temporary, path)
        finally:
            Path(temporary).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--environment", required=True)
    parser.add_argument("--spec", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--dep-file")
    parser.add_argument("--incremental-directory")
    args = parser.parse_args()
    tools = load(args.environment)
    root = Path.cwd()
    env = native_environment(tools)
    payload = load(args.spec)
    output = Path(args.output).absolute()
    incremental = payload["unit"]["mode"] != "run-custom-build" and payload["unit"]["profile"]["incremental"]
    if incremental != (args.incremental_directory is not None):
        raise RuntimeError("incremental directory must match the Cargo unit profile")
    incremental_directory = None
    if incremental:
        incremental_directory = Path(args.incremental_directory).absolute()
        if incremental_directory == output or incremental_directory.is_relative_to(output) or output.is_relative_to(incremental_directory):
            raise RuntimeError("incremental state must be separate from ordinary output")
        # Buck retains all outputs for these actions. Preserve only compiler
        # state, never a prior library, executable, or source dependency list.
        if output.exists():
            shutil.rmtree(output)
        if args.dep_file:
            Path(args.dep_file).unlink(missing_ok=True)
        incremental_directory.mkdir(parents=True, exist_ok=True)
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
        try:
            if incremental_directory is not None:
                own_readonly_incremental_files(incremental_directory, env)
            compile_unit(payload, output, env, tools, root, args.dep_file, incremental_directory)
        except BaseException:
            if incremental_directory is not None:
                if output.exists():
                    shutil.rmtree(output)
                if args.dep_file:
                    Path(args.dep_file).unlink(missing_ok=True)
            raise
    info = {"filename": payload["unit"]["filename"], "library_paths": library_paths}
    temporary = test_tmp_path(payload["unit"])
    if temporary is not None:
        info["test_tmp"] = str(temporary)
    (output / "unit.json").write_text(json.dumps(relocate(info, root, output, encode=True), sort_keys=True))


if __name__ == "__main__":
    main()
