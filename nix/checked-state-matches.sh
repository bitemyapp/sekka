# Sourced by the package checks that run Clippy over typestate machines.
# Prove that the restriction lint is active with the same -A clippy::all
# used below: rustc alone silently accepts these Clippy attributes.
checked_state_matches=$(mktemp -d)
cat >"$checked_state_matches/exhaustive.rs" <<'RS'
#![deny(clippy::wildcard_enum_match_arm)]
pub enum State { Preparing, Writing, Done }
pub fn active(state: State) -> bool {
    match state {
        State::Preparing => false,
        State::Writing | State::Done => true,
    }
}
RS
sed 's/State::Writing | State::Done/_/' "$checked_state_matches/exhaustive.rs" >"$checked_state_matches/wildcard.rs"
if ! clippy-driver --edition=2021 --crate-type lib --emit=metadata -A clippy::all \
    "$checked_state_matches/exhaustive.rs" -o "$checked_state_matches/exhaustive.rmeta" ||
  clippy-driver --edition=2021 --crate-type lib --emit=metadata -A clippy::all \
    "$checked_state_matches/wildcard.rs" -o "$checked_state_matches/wildcard.rmeta" \
    >"$checked_state_matches/error" 2>&1 ||
  ! grep -q 'clippy::wildcard_enum_match_arm' "$checked_state_matches/error"; then
  cat "$checked_state_matches/error" >&2
  echo "checked-state-matches: Clippy must accept exhaustive matches and reject wildcard arms" >&2
  exit 1
fi
rm -rf "$checked_state_matches"
