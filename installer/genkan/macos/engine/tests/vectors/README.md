# Engine vectors

Documents and transcripts the engine wrote against the fake Mac in
`tests/support.py`, and `expected.json`: the canonical bytes and digests the
Rust contract crate computes for the same documents.

- `tests/test_canonical.py` checks that Python writes `expected.json`'s bytes
  and digests.
- `tests/test_vectors.py` checks that the engine still writes these
  transcripts, byte for byte.
- `crates/contract/tests/engine_vectors.rs` checks the documents with the
  Rust types and feeds every transcript to the helper's `Transcript`.
- `macos_margin` in `expected.json` isn't computed: it is the room a
  free-space plan keeps in macOS, which `plan.rs` and the engine's
  `contract.py` each check theirs against.

To regenerate after a deliberate change (from `installer/genkan/macos`):

```sh
cargo build -p sekka-mac-contract --examples
SEKKA_CANONICALIZE=$PWD/target/debug/examples/canonicalize \
SEKKA_ASAHI_ROOT=/path/to/extracted/installer-v0.9.2 \
  python3 -I -B engine/tests/make_vectors.py
```

Review the diff: a changed transcript is a changed protocol or behavior.
