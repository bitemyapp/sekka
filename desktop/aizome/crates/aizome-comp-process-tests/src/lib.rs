//! Tests (in `tests/`) that run a built `aizome` binary, named by
//! `AIZOME_TEST_COMPOSITOR`, as a process. They depend on nothing but the
//! IPC client, so the package checks the compositor it ships, as built,
//! rather than one rebuilt with `aizome-comp`'s test dependencies (whose
//! features change the core's symbols).
