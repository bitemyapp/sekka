# Unresolved inner `macro_use` fixture

`probe.rs` imports macros through an inner attribute in a file module;
`inline.rs` uses the same pattern in an inline module. Both are valid Rust.
Run from this directory, with output/cache paths outside the checkout:

```sh
rustc --edition=2021 --test probe.rs -o /path/to/evidence/probe
/path/to/evidence/probe
rustc --edition=2021 --test inline.rs -o /path/to/evidence/inline
/path/to/evidence/inline
MAKEPAD_RUN_CACHE_DIR=/path/to/evidence/cache /path/to/relax/target/release/relax test probe.rs
MAKEPAD_RUN_CACHE_DIR=/path/to/evidence/cache /path/to/relax/target/release/relax test inline.rs
```

Set `MAKEPAD_RUN_STD` to the matching Relax checkout's
`libs/relax/compiler/std`. The Relax commands are expected to fail on the pinned
experiment compiler. The outer-attribute form (`#[macro_use] mod macros;`)
passes the separate counterfactual probe retained with the remote evidence.
This fixture records a limitation; it is not a workaround for Sekka dependencies.
