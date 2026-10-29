# Semantic prompt metadata and screen alignment

Hotaru retains OSC 133 content and row markers in its Rust core. The observed
A/B/C/D/I/L/N/P actions include fresh-line behavior, prompt-kind options,
explicit versus automatic line endings, alternate-screen cursor semantics,
prompt-preserving ED2, and metadata through edits, reflow, compressed history
and retained snapshots. This supports future semantic selection and command
navigation. Those native features, command tracking, redraw policy and complete
shell integration extensions remain unfinished; the existing shell hooks still
emit only cwd information.

Cells remain eight bytes. Two previously unused integer flag bits hold Output,
Input or Prompt; no enum or pointer is written by the packed ASCII store. Its
template is constructed once per run, with unchanged exclusive-cell bounds and
alignment invariants. Rows separately record None, Prompt or Continuation.
History encodes both fields and rejects invalid flag values. Mutations preserve
copy-on-write ownership. Read-only cell accessors distinguish printed spaces,
unprinted blanks and wide-character wrap padding.

An independent audit exposed the previously absent DECALN (`ESC # 8`). It fills
the active screen with plain E cells, retains pen foreground/background while
clearing other SGR attributes, resets margins/origin/cursor/wrap, and preserves
history and current cursor protection/link/semantic state. Pattern cells do not
inherit those cursor attributes. Earlier retained snapshots remain valid.

The first semantic candidate scanned rows on every ED2, including programs that
had never sent OSC 133. A conservative terminal-wide `semantic_markers_seen`
hint now bypasses that scan until a valid marker arrives. It stays set through
soft reset, alternate-screen switches and reflow; RIS replaces all state. The
hint can retain an unnecessary scan, but cannot suppress an existing marker.

## Reference and correctness evidence

All execution ran on `wx-workstation`. The unchanged reference is Ghostty
`6ba144158577d033ccb6ed4b574e4013957c9161`; its actual VT library hash is
`70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639`.
The C adapter calls that library; it is development tooling, not a product
dependency. It observes retained and visible cells, semantic content, width,
protection, row markers and cursor state after feed/resize/compression steps.

- 610 actual-library expectations replay at three input chunk sizes: 1,830
  comparisons in the ordinary test suite. Initial alternate-screen and ED2
  failures are retained in the evidence directory.
- The independent final audit passes 3,120 comparisons, including empty prompts,
  edited/wide/grapheme cells, protected erasure, cold history, reflow, DECALN and
  the marker guard's soft-reset/alternate/RIS cases. The original 54 DECALN
  failures and their corrected 3,090-case run remain separately archived.
- Another 54 actual-library common-state comparisons qualify DECALN's colors,
  attributes, margins and modes on the pre-guard binary. Its alignment helper
  is unchanged in the final source.
- The final workspace passes 160 tests in both debug and release, with two
  existing ignored tests, formatting and strict all-target Clippy. Four packed
  ASCII-store tests passed strict-provenance Miri on the same semantic-store
  implementation before the later DECALN/guard changes.

See [`upstream/semantic-prompts/`](../upstream/semantic-prompts/) for exact
source/adapter hashes, runners, failures and raw results. The final semantic
adapter is `ed0b72771b35ba894d2a97954085c1bc71104d13a7b6092c4df962c5eb59c816`.
The actual Ghostty C adapter is
`f78b17b7c9c9c7b06dc3c781596dda6574d83b15e7b04ca76d1b93cbf747aa4b`.

## Measured cost of the feature

The final native source digest is
`54ceb66cb7fde4d5edcccfd66b32c588f7af4a02cf381b7846d5c5036ed05267`;
its replay ELF is
`23967d024171bc0403d6280c53e4692e203e981371d7a57b0d0fefc0a4cea2d5`.
Rust 1.95, native CPU targeting, opt-level 3, thin LTO and one codegen unit
match the unchanged qualified Rust baseline. All three engines ran on CPU 24,
with SMT sibling 88 observed, under the shared heavy-job lock. Each case used
three warmups and 60 samples per engine across all six execution orders.

All 40 baseline/candidate full-state and actual-Ghostty common-state gates pass.
The common throughput inputs contain no OSC 133: they measure the cost imposed
on existing paths, not throughput of the new prompt behavior. Median
candidate/baseline ratios span 0.9632–1.0383. The separate large-input subset uses
120×80 cells and about 8 MiB; the smaller/malformed cases use 120×12 and about
2 MiB. Ratios below one are faster:

| Large workload | Candidate / Rust baseline, legacy / Unicode | Candidate / Ghostty, legacy / Unicode |
| --- | --- | --- |
| ASCII | 1.0090 / 1.0156 | 0.8574 / 0.8574 |
| Mixed Unicode | 0.9990 / 1.0168 | 1.0807 / 1.0890 |
| Styled text | 0.9837 / 0.9935 | 0.8222 / 0.8190 |
| Cursor controls | 1.0028 / 0.9891 | 0.1221 / 0.1211 |

The runner permits unrelated work on other workstation cores and records it.
Six workload windows observed one 10 ms kernel-work tick each on CPU 24; their
flags remain in the report. Sibling activity stayed below 0.04% in those windows.
Process sampling cannot exclude unsampled migrations. These are bounded
feature-cost observations, not proof of a small speedup or a perfectly idle host.
The first attempt used a whole-host compiler gate and aborted after 15 cases;
its unguarded ED2 showed an indicative roughly twofold cursor cost. That failed
attempt is retained and is not acceptance evidence.

The [performance archive](../upstream/semantic-prompts/performance/summary.json)
includes losslessly compressed raw timings, all observations and exact source,
toolchain, input and executable identities. No GUI, retained-memory or whole-app
parity claim follows from these zero-history replay measurements. Mixed Unicode,
malformed-input and retained-history memory gaps remain unresolved. The previous
442-sprite Nix/native checkpoint still identifies its earlier core; this change
needs a later combined package qualification.
