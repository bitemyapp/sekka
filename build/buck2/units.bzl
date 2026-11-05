"""One Buck action per Cargo unit; Cargo resolves, Buck schedules and caches."""

CargoUnitInfo = provider(fields = {
    "output": provider_field(Artifact),
    "closure": provider_field(dict),
    "spec": provider_field(dict),
})

SourceInfo = provider(fields = {
    "path": provider_field(typing.Any),
    "inputs": provider_field(list),
})

def _source_impl(ctx):
    if ctx.attrs.store_path:
        # Nix store paths encode immutable source content. They are available in
        # the local sandbox and in the read-only worker store, not host caches.
        path = ctx.attrs.store_path
        outputs = []
    else:
        path = ctx.actions.symlinked_dir("source", ctx.attrs.files)
        outputs = [path]
        if ctx.attrs.prefix:
            path = path.project(ctx.attrs.prefix)
    return [DefaultInfo(default_outputs = outputs), SourceInfo(path = path, inputs = outputs)]

cargo_source = rule(
    impl = _source_impl,
    attrs = {
        "files": attrs.dict(attrs.string(), attrs.source(), default = {}),
        "store_path": attrs.string(default = ""),
        "prefix": attrs.string(default = ""),
    },
)

def _unit_impl(ctx):
    spec = json.decode(ctx.attrs.spec)
    closure = {}
    dependencies = []
    for dep in ctx.attrs.deps:
        info = dep[CargoUnitInfo]
        closure.update(info.closure)
        closure[str(dep.label.raw_target())] = info.output
        dependencies.append({"directory": info.output, "spec": info.spec})
    output = ctx.actions.declare_output("unit", dir = True)
    payload = {
        "unit": spec,
        "source": ctx.attrs.source[SourceInfo].path,
        "dependencies": dependencies,
        "closure": closure.values(),
        "extra_sources": {key: value[SourceInfo].path for key, value in ctx.attrs.extra_sources.items()},
    }
    manifest = ctx.actions.write_json("action.json", payload, with_inputs = True)
    python = read_root_config("sekka", "python")
    environment = read_root_config("sekka", "environment")
    if not python or not environment:
        fail("Use sekka-buck2 from the root-lock Nix shell (missing immutable environment).")
    ctx.actions.run(
        cmd_args(
            python,
            ctx.attrs._runner,
            "--environment",
            environment,
            "--spec",
            manifest,
            "--output",
            output.as_output(),
            # A projected crate path omits declared sibling resources from the
            # remote input tree. Retain the complete, narrowly assembled tree.
            hidden = [ctx.attrs.source[SourceInfo].inputs] + [source[SourceInfo].inputs for source in ctx.attrs.extra_sources.values()],
        ),
        category = "cargo_" + spec["mode"].replace("-", "_"),
        identifier = spec["package"]["name"] + "-" + spec["target"]["name"],
        allow_cache_upload = True,
    )
    executable = output.project(spec["filename"]) if spec["filename"] else None
    providers = [
        DefaultInfo(default_output = executable if executable else output, other_outputs = [output]),
        CargoUnitInfo(output = output, closure = closure, spec = spec),
    ]
    runtime = read_root_config("sekka", "runtime", "")
    run = cmd_args(python, ctx.attrs._runtime, "--action-module", ctx.attrs._runner, "--environment", environment, "--runtime", runtime, "--run", output)
    if spec["mode"] == "test":
        providers.append(ExternalRunnerTestInfo(type = "rust", command = [cmd_args(run, "--test-run", "--")], env = {}, labels = [], contacts = [], run_from_project_root = True, use_project_relative_paths = True))
    elif "bin" in spec["target"]["crate_types"] and spec["mode"] == "build":
        providers.append(RunInfo(args = cmd_args(run, "--")))
    return providers

cargo_unit = rule(
    impl = _unit_impl,
    attrs = {
        "spec": attrs.string(),
        "source": attrs.dep(providers = [SourceInfo]),
        "deps": attrs.list(attrs.dep(providers = [CargoUnitInfo]), default = []),
        "extra_sources": attrs.dict(attrs.string(), attrs.dep(providers = [SourceInfo]), default = {}),
        "_runner": attrs.default_only(attrs.source(default = "root//build/buck2:action.py")),
        "_runtime": attrs.default_only(attrs.source(default = "root//build/buck2:run.py")),
    },
)
