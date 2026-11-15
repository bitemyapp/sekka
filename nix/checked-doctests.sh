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
#
# It prints nothing when it finds none, so a check that only loops over the
# names tests nothing and passes, as it would after the source moved.
# Compare the names with the list the check expects, and stop when they
# differ. A doctest whose package can't be named is a failure: in a command
# substitution that is the substitution's status.
compile_fail_packages() {
  local files file dir name packages=
  files=$(grep -rlF --include='*.rs' '```compile_fail' "$@" 2>/dev/null || true)
  while read -r file; do
    [ -n "$file" ] || continue
    dir=$(dirname "$file")
    until [ -f "$dir/Cargo.toml" ] || [ "$dir" = . ] || [ "$dir" = / ]; do dir=$(dirname "$dir"); done
    name=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$dir/Cargo.toml" 2>/dev/null | head -n1) || true
    if [ -z "$name" ]; then
      echo "checked-doctests: $file has a compile_fail doctest, but its package has no name to test it by" >&2
      return 1
    fi
    packages+="$name"$'\n'
  done <<<"$files"
  printf '%s' "$packages" | sort -u
}

# It must name the package that has one and no other, and fail where a
# package has no name.
mkdir -p "$checked_doctests"/{with/src,without/src,nameless/src}
printf '[package]\nname = "with-one"\n' >"$checked_doctests/with/Cargo.toml"
printf '[package]\nname = "without-one"\n' >"$checked_doctests/without/Cargo.toml"
printf '//! ```compile_fail,E0425\n//! missing;\n//! ```\n' | tee "$checked_doctests/with/src/lib.rs" >"$checked_doctests/nameless/src/lib.rs"
printf '//! ```\n//! let _ = 1;\n//! ```\n' >"$checked_doctests/without/src/lib.rs"
if [ "$(cd "$checked_doctests" && compile_fail_packages with without 2>/dev/null)" != with-one ] ||
  [ -n "$(cd "$checked_doctests" && compile_fail_packages without 2>/dev/null)" ] ||
  (cd "$checked_doctests" && compile_fail_packages with nameless) >/dev/null 2>&1; then
  echo "checked-doctests: compile_fail_packages doesn't name the packages to test" >&2
  exit 1
fi
