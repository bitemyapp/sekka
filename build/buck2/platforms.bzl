"""Local, authenticated cache, or remote execution on one stable Nix platform."""

def _execution_impl(ctx):
    mode = read_root_config("sekka", "execution", "local")
    if mode not in ["local", "cache", "remote"]:
        fail("sekka.execution must be local, cache, or remote")
    write = read_root_config("sekka", "cache_write", "false") == "true"
    properties = {"OSFamily": "linux", "ISA": "x86-64"}
    if mode != "local":
        runtime = read_root_config("sekka", "worker_runtime", "")
        if not runtime:
            fail("Regenerate the service client config after migrating its worker runtime.")
        properties["sekka.worker-runtime"] = runtime
    platform = ExecutionPlatformInfo(
        label = ctx.label.raw_target(),
        configuration = ConfigurationInfo(constraints = {}, values = {}),
        executor_config = CommandExecutorConfig(
            local_enabled = mode != "remote",
            remote_enabled = mode == "remote",
            remote_cache_enabled = mode != "local",
            allow_cache_uploads = mode != "local" and write,
            remote_execution_properties = properties,
            remote_execution_use_case = "sekka",
            remote_output_paths = "output_paths",
            use_limited_hybrid = False,
        ),
    )
    return [DefaultInfo(), ExecutionPlatformRegistrationInfo(platforms = [platform])]

execution = rule(impl = _execution_impl, attrs = {})
