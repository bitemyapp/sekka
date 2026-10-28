# Identity-based partial scroll reuse

The component patch applies to the 178-sprite runtime at `ba6cf44`. Its three
source files match `result.json`; `source-manifest.json` records all 228
available runtime build inputs, including applied Sidestep sources. The five
tracked patch inputs are absent from the isolated source tree because its
Sidestep copy already includes them. The gitlink is unchanged in product.

`qualified-tests.log` records 62 passing native tests and seven existing ignores.
`qualified-clippy.log` is the strict all-target check. `native-wayland.json`
records every full-window RGBA hash for 42 paired captures using the same
binary with pixel copying enabled/disabled. Two original-resolution images are
retained here. All captures, traces, initial fixture mistakes and superseded
full-rectangle/bounded runs remain at
`/home/callen/work/hotaru-region-scroll-isolated/artifacts/region-scroll/`
locally and on `wx-workstation`; the qualified ELF remains on the workstation.

`baseline.log` is the actual pre-change missing-region-reuse failure after
fixture setup was corrected. `exposure-gate-red.log` is the later review
counterexample: two unique rows move across many shared stationary rows, and
exposed rows must not count as retained pixels. Both regressions pass in the
qualified source. The reviewer also identified the original full-window-copy
cost for a four-row region; the final interval copy and work-count assertions
cover it. Initial fixture setup failures are not evidence against the product.

For reproduction, prepare `ba6cf44` plus `candidate.patch` in an isolated
workstation checkout with the tracked Sidestep patches applied, then invoke
`bash apps/hotaru/upstream/region-scroll/reproduce.sh SOURCE EVIDENCE` through
`ssh wx-workstation`. The script validates all recorded runtime source hashes
before building and holds the shared workstation lock. Generated binaries and
captures stay outside Git. Later combined builds require their own source record.

This is correctness and damage-work qualification. A row-count gate does not
prove a universal frame-time improvement; memory-copy cost, renderer scheduling
and the earlier software GUI pilot's 60 Hz scrolling result remain to measure.
