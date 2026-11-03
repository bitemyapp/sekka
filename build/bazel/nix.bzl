"""Toolchains and native libraries from the root flake's Nixpkgs.

rules_nixpkgs builds nix/bazel/toolchains.nix with nix/bazel/nixpkgs.nix as
`<nixpkgs>` (the root flake's own input, read from flake.lock), so these are
the store paths `nix develop .#aizome` and the Aizome package use. Building
them needs `nix-build` on PATH; actions only read the results.
"""

load("@rules_nixpkgs_cc//:cc.bzl", "nixpkgs_cc_configure")
load("@rules_nixpkgs_core//:nixpkgs.bzl", "nixpkgs_local_repository", "nixpkgs_package")
load("@rules_nixpkgs_rust//:rust.bzl", "nixpkgs_rust_configure")

# Bounded, as the build plan starts Nix preparation on the shared workstation.
_NIXOPTS = ["--max-jobs", "2", "--cores", "4"]

# As canonical label strings, which nixpkgs_package takes.
_TOOLCHAIN_FILES = [str(Label(f)) for f in [
    "//nix/bazel:toolchains.nix",
    "//nix/bazel:aizome-inputs.nix",
    "//nix:libsecret-dh-padding.nix",
    "//nix:patches/libsecret-gnutls-dh-padding.patch",
    "//desktop/aizome/nix:fonts.nix",
    "//desktop/aizome/nix:aizome-fonts.nix",
    "//desktop/aizome/nix:hotaru-fonts.nix",
]]

def _nix_impl(module_ctx):
    nixpkgs_local_repository(
        name = "nixpkgs",
        nix_file = Label("//nix/bazel:nixpkgs.nix"),
        nix_file_deps = [Label("//:flake.lock")],
    )
    nixpkgs_cc_configure(
        name = "nix_cc",
        repository = "@nixpkgs",
        nix_file_content = "(import ./nix/bazel/toolchains.nix { }).cc",
        nix_file_deps = _TOOLCHAIN_FILES,
        nixopts = _NIXOPTS,
        register = False,
    )
    nixpkgs_rust_configure(
        name = "nix_rust",
        repository = "@nixpkgs",
        nix_file_content = "(import ./nix/bazel/toolchains.nix { }).rust",
        nix_file_deps = _TOOLCHAIN_FILES,
        nixopts = _NIXOPTS,
        register = False,
    )
    nixpkgs_package(
        name = "nix_native",
        repository = "@nixpkgs",
        nix_file_content = "(import ./nix/bazel/toolchains.nix { }).native",
        nix_file_deps = _TOOLCHAIN_FILES,
        nixopts = _NIXOPTS,
    )
    nixpkgs_package(
        name = "nix_test_env",
        repository = "@nixpkgs",
        nix_file_content = "(import ./nix/bazel/toolchains.nix { }).testEnv",
        nix_file_deps = _TOOLCHAIN_FILES,
        nixopts = _NIXOPTS,
    )
    return module_ctx.extension_metadata(reproducible = False)

nix = module_extension(implementation = _nix_impl)
