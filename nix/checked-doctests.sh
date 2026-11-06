# Sourced in a Rust package's check phase, before `cargo test`.
#
# A doctest marked ```compile_fail,E0599 must fail with that error, but
# rustdoc checks the code only when it runs as nightly: on stable, any error
# passes the test, a typo or a missing import included. RUSTC_BOOTSTRAP=1
# makes it nightly, for rustdoc alone. Given to cargo, it would also reach
# build scripts that probe for nightly features, and everything would
# rebuild.
checked_doctests=$(mktemp -d)
cat >"$checked_doctests/rustdoc" <<EOF
#!$(command -v bash)
RUSTC_BOOTSTRAP=1 exec $(command -v rustdoc) "\$@"
EOF
chmod +x "$checked_doctests/rustdoc"
export RUSTDOC=$checked_doctests/rustdoc

# The wrong code must fail and the right one pass, or no compile_fail test's
# code is being checked.
for code in E0308 E0425; do
  cat >"$checked_doctests/expect_$code.rs" <<EOF
//! \`\`\`compile_fail,$code
//! let x: u8 = missing;
//! \`\`\`
EOF
done
if "$RUSTDOC" --test --edition 2021 "$checked_doctests/expect_E0308.rs" >/dev/null 2>&1 ||
  ! "$RUSTDOC" --test --edition 2021 "$checked_doctests/expect_E0425.rs" >/dev/null 2>&1; then
  echo "checked-doctests: rustdoc isn't checking compile_fail error codes" >&2
  exit 1
fi

# The packages under the given directories with a doctest that must not
# compile, one name per line. Test them one at a time: tested together,
# cargo gives each the features the others' tests turn on, and a doctest
# that shows a shipped build lacks a test-only feature is skipped.
compile_fail_packages() {
  { grep -rlF --include='*.rs' '```compile_fail' "$@" || true; } | while read -r file; do
    dir=$(dirname "$file")
    until [ -f "$dir/Cargo.toml" ] || [ "$dir" = . ]; do dir=$(dirname "$dir"); done
    echo "$dir"
  done | sort -u | while read -r dir; do
    sed -n 's/^name = "\(.*\)"$/\1/p' "$dir/Cargo.toml" | head -n1
  done
}
