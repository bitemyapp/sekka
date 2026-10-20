# Run in the user's service manager, never in the privileged installer.
# Explicit --toolchain arguments keep project overrides out of this setup.
rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"
pending="$rustup_home/.sekka-initializing"
if [[ ! -e "$rustup_home/settings.toml" ]]; then
    mkdir -p "$rustup_home"
    touch "$pending"
fi

# A minimal profile is an explicit choice to leave editor tools out.
profile=$(rustup show profile)
if [[ "$profile" == minimal ]]; then
    exit 0
fi

if toolchain=$(rustup default 2>&1); then
    toolchain=${toolchain% (default)}
    # `rustup toolchain link` creates symlinks; custom toolchains manage
    # their own components and must not be replaced with stable.
    if [[ -L "$rustup_home/toolchains/$toolchain" ]]; then
        exit 0
    fi
elif [[ "$toolchain" == 'error: no default toolchain is configured' ]]; then
    if [[ ! -e "$pending" ]]; then
        # Preserve `rustup default none`, including accounts with only
        # project-specific toolchains.
        exit 0
    fi
    rustup toolchain install stable --profile default \
        --component rust-analyzer --component rust-src
    # Rustup selects the first installed toolchain as the default itself.
    rm -f "$pending"
    exit 0
else
    printf '%s\n' "$toolchain" >&2
    exit 1
fi

# Idempotent and offline when both are installed. This also repairs accounts
# created before Sekka included editor support, without updating their compiler.
rustup component add --toolchain "$toolchain" rust-analyzer rust-src
rm -f "$pending"
