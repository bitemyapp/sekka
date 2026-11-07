# Storage plan at the single-CAS tip, 2026-11-06

The 31-stage storage plan of the [storage run](../2026-11-06-storage-plan/README.md),
run once more on `wx-workstation` (128 logical CPUs, Btrfs `compress=zstd:1`).
It used `feat/buck2-single-cas` at `7fef66a8` on a fresh service with a
`--local-cas` writer client: local execution, hits cloned from the CAS, and
produced outputs journaled and shared with it. Stale cleaning was configured
but does not run within a plan (its first pass is 12 hours after daemon
start). [config.json](config.json), [summary.json](summary.json) and
[storage.json](storage.json) hold the frozen configuration, the harness
summary, and every stage's unions and attribution.

The fixes this run adds over the [single-CAS run](../2026-11-06-single-cas/README.md):
- the paused-gateway restart (`f3b08b82`);
- one CAS scan per post-build pass (`a07aaff7`);
- the engine index rescanned outside its lock (`4f41ec88`);
- shared placeholder copies (`54ff881a`);
- threaded sharing (`1cfc5199`);
- stale cleaning (`1356dfba`).

Fixes from a later review (`bdc406ca`: journal lock and leftovers, client
journal gating, controller signal handling) came after it.

| Gate | This run | Single-CAS run | Storage run |
| --- | --- | --- | --- |
| Correctness | pass: 31 observations, no failed builds or native retries | pass | pass |
| Physical sharing, per added worktree | pass: +14.0, +14.5, +14.2, +14.8 MB (limit 455 MB) | +20.5 to +21.8 MB | −1,054 to +168 MB |
| Warm worktree ≤20% of cold | pass: 4.6–5.3% | 5.8–7.2% | fail: 15–23% |
| No-op | pass: 0.21 s | 0.22 s | 0.16 s |
| Restart, primed output loss | pass | pass | pass |
| Eviction recovery | **pass** | fail: controller bug | pass |
| Edits (reported) | leaf 1.6 s, shared 15.8 s, icon 16.3 s | 2.0, 19.4, 16.8 s | 1.7, 16.5, 17.3 s |

Active physical union (compressed bytes, one `compsize` pass over every owned
root) at selected stages:

| Stage | This run | Single-CAS run | Storage run |
| --- | --- | --- | --- |
| dev-cold (83.6 s) | 8.46 GB | 9.34 GB | 13.23 GB |
| dev-revert, after three edits | 12.86 GB | 13.70 GB | 21.84 GB |
| dev-fresh-e, five worktrees | 12.92 GB | 13.79 GB | 21.02 GB |
| dev-divergent-j4, five at once (34.7 s makespan) | 23.23 GB | 23.83 GB | 32.83 GB |
| service-eviction, CAS resized to 4 GiB | 15.51 GB | not evicted | 19.50 GB |
| release-fresh, all 742 cached (3.7 s) | 19.20 GB | 29.31 GB | 26.16 GB |
| final-original | 20.16 GB | 30.51 GB | 28.52 GB |

The final union is 29% below the storage run's, with the same stages and
the same eviction. The single-CAS run's later stages ran on an unevicted
cache, so its figures there are not comparable.

After the review fixes, at `bdc406ca` on wx:
- `checks.x86_64-linux.buck2-backend` passed, now including leftover-journal
  and late-append cases;
- `checks.x86_64-linux.build-service-inventory` passed, now including gateway
  resume and eviction-during-scan cases;
- through a `--local-cas` client, `outputs` for Soroban (243 actions) printed
  its JSON and its pass shared 1,920 outputs, and `runtime` staged two
  launchers;
- a `--cache local --cache-write` build wrote no journal and kept its
  outputs in the content store (1.57 GB).

These are workstation build and storage observations, not image, installer
or hardware qualification. No broad flake check was run.
