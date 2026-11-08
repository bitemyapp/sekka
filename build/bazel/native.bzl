"""Aizome's native build environment as Make variables.

A build script that finds C libraries with pkg-config, or generates bindings
with bindgen, names this target in its toolchains and reads `$(NAME)` in its
environment: MODULE.bazel's annotations for crates.io's crates,
build/bazel/generate.py for Sekka's own. The values are the root flake's
store paths (nix/bazel/toolchains.nix).
"""

load("@nix_native//:env.bzl", "NATIVE_ENV")
load("@nix_test_env//:env.bzl", "TEST_ENV")

def _native_env_impl(_ctx):
    return [platform_common.TemplateVariableInfo(dict(NATIVE_ENV))]

native_env = rule(
    implementation = _native_env_impl,
    doc = "Make variables of the native environment (nix/bazel/toolchains.nix).",
)

def _test_env_impl(_ctx):
    return [platform_common.TemplateVariableInfo({"TEST_" + k: v for k, v in TEST_ENV.items()})]

test_env = rule(
    implementation = _test_env_impl,
    doc = "Make variables `$(TEST_NAME)` of the tests' environment (nix/bazel/toolchains.nix).",
)

# What every test of Sekka's crates runs with (build/bazel/generate.py).
TEST_ENV_VARIABLES = sorted(TEST_ENV)
