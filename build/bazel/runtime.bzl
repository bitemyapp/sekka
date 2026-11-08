"""Run current Bazel binaries with their declared Nix runtime and logic dylib."""

load("@rules_rust//rust:defs.bzl", "rust_binary")
load("@nix_test_env//:env.bzl", "TEST_ENV")

def _quote(value):
    return "'" + value.replace("'", "'\"'\"'") + "'"

def _runtime_impl(ctx):
    executable = ctx.actions.declare_file(ctx.label.name)
    binary = ctx.executable.binary
    files = [binary]
    parents = "/".join([".."] * len(ctx.label.package.split("/")))
    lines = [
        "#!/bin/sh",
        "set -eu",
        'case "$0" in */*) own=${0%/*} ;; *) own=. ;; esac',
        'own=$(CDPATH= cd -- "$own" && pwd)',
        'if [ -d "$0.runfiles" ]; then',
        '  RUNFILES_DIR=$(CDPATH= cd -- "$0.runfiles" && pwd); export RUNFILES_DIR',
        '  workspace="$RUNFILES_DIR/' + ctx.workspace_name + '"',
        'elif [ -x "$own/' + binary.basename + '" ]; then',
        '  workspace=$(CDPATH= cd -- "$own/' + parents + '" && pwd)',
        'elif [ -n "${RUNFILES_DIR:-}" ]; then',
        '  workspace="$RUNFILES_DIR/' + ctx.workspace_name + '"',
        'else echo "Missing declared Bazel runtime for $0" >&2; exit 1; fi',
    ]
    for key, value in sorted(TEST_ENV.items()):
        if key == "PATH":
            lines.append("export PATH=" + _quote(value) + ':"${PATH:-}"')
        else:
            lines.append("export " + key + "=" + _quote(value))
    for tool in ctx.attr.runtime_bins:
        tool_binary = tool[DefaultInfo].files_to_run.executable
        files.append(tool_binary)
        directory = tool_binary.short_path.rsplit("/", 1)[0]
        lines.append('export PATH="$workspace/' + directory + ':$PATH"')
    if ctx.attr.logic:
        logic_files = ctx.attr.logic[DefaultInfo].files.to_list()
        libraries = [f for f in logic_files if f.basename.endswith(".so")]
        if len(libraries) != 1:
            fail("Expected exactly one logic dylib, got %s" % libraries)
        files += logic_files
        lines.append('export AIZOME_LOGIC="$workspace/' + libraries[0].short_path + '"')
    lines.append('exec "$workspace/' + binary.short_path + '" "$@"')
    ctx.actions.write(executable, "\n".join(lines) + "\n", is_executable = True)
    runfiles = ctx.runfiles(files = files).merge(ctx.attr.binary[DefaultInfo].default_runfiles)
    if ctx.attr.logic:
        runfiles = runfiles.merge(ctx.attr.logic[DefaultInfo].default_runfiles)
    for tool in ctx.attr.runtime_bins:
        runfiles = runfiles.merge(tool[DefaultInfo].default_runfiles)
    # Aggregate builds must materialize executable dependencies too, even
    # when Bazel does not construct a runfiles tree for each nested binary.
    # These are declared current outputs, never a search for old artifacts.
    return [DefaultInfo(
        executable = executable,
        files = depset([executable], transitive = [runfiles.files]),
        runfiles = runfiles,
    )]

_runtime = rule(
    implementation = _runtime_impl,
    executable = True,
    attrs = {
        "binary": attr.label(executable = True, cfg = "target", mandatory = True),
        "logic": attr.label(),
        "runtime_bins": attr.label_list(),
    },
)

def sekka_rust_binary(name, logic = None, runtime_bins = [], **kwargs):
    rust_binary(name = name + "__bin", **kwargs)
    _runtime(name = name, binary = ":" + name + "__bin", logic = logic, runtime_bins = runtime_bins)
