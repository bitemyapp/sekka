# xdg-mime desktop queries

Submitted as [ebassi/xdg-mime-rs#39](https://github.com/ebassi/xdg-mime-rs/pull/39)
on 2026-11-03. The seven-commit [patch series](desktop-correctness.patch) applies
to upstream `f8efb3fb1fad582539e2af7d1e9fbc9613dcc7d9` with `git am`.
Our fork revision is `8907f908b49ca5537f4f67365c4e2b7c723911df`.

Aizome needs shared-mime-info names, content detection, aliases, inheritance,
icons, descriptions and user overrides. GLib did not honor `glob-deleteall`
across database directories. Testing the Rust `xdg-mime` backend exposed the
same issue and additional correctness gaps. The series fixes:

- Filename and magic delete-all markers, preserving same-directory replacements
  and higher-precedence additions.
- Case flags, malformed globs, duplicate rule precedence, literal priority and
  deterministic ambiguous results. Literal and suffix indexes replace the full
  database scan for ordinary filename queries.
- Global magic priority, nested alternatives and host-endian words, with safe
  handling of empty match values and read failures.
- Missing versus empty content, plain-text fallback and cyclic inheritance.
- Reloads after in-place index updates, directory creation and removal.

It also adds isolated directory sets and cached localized descriptions from
generated per-type XML, using `roxmltree`. The binary `mime.cache` format is
still unsupported; the backend reads the other indexes installed alongside it.

Validation at the pinned revision: 55 unit tests, three integration tests and
eight doctests pass. Clippy completes with upstream style warnings. Aizome's
component tests additionally compile isolated databases with `update-mime-database`
to check overrides, weights, aliases, descriptions, ambiguous content and reloads.
These are not full specification conformance, fuzzing or desktop/ISO validation.

A local release-mode timing used the installed shared-mime-info database and
100,000 filename queries, cycling through `photo.PNG`, `README`, `archive.tar.gz`,
`main.rs`, `report.pdf`, `unknown`, `report-42.txt`, `song.flac`, `movie.mkv` and
`image.jpeg`. Upstream took 4.480 s for queries and 6.676 ms to load; the patched
backend took 37.860 ms and 5.886 ms respectively. This is a local microbenchmark,
not an application benchmark or a CI timing threshold.

`desktop/aizome/crates/aizome-mime/Cargo.toml` pins the fork. Once upstream
contains the series, switch that dependency to an upstream revision or release,
update Aizome's Cargo lock and `nix/cargo-output-hashes.nix`, and rerun the MIME
and dependent app checks. Keep the regression tests when retiring the fork.
