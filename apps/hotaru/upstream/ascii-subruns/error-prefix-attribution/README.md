# Error-prefix attribution after rejection

This is diagnostic evidence for the rejected error-prefix partition, not a
new implementation or a native performance claim. Shared product core remains
unchanged. All execution was on wx-workstation under the shared lock.

The qualified baseline source is 7e36bc31; the rejected candidate is ba30dfe4.
Complete hashes, vendor/source manifests and compiler settings are in
summary.json and diagnostic-build.json. Both diagnostic builds use Rust 1.95,
x86-64-v3, optimization 3, thin LTO, one codegen unit and debug 2. The baseline
reuses its verified immutable earlier diagnostic build. The candidate ELF is
`49ac9d50344b415553290190182d7b3efe6055533cd79e08ac5f1a0379d1613d`.
The original native ELFs remain unchanged and are separately identified for
disassembly. Native ISA and v3 diagnostic instruction streams are not assumed
identical.

Three bounded legacy-mode inputs use 120 columns × 12 rows, zero history and 64 KiB
chunks. Each full baseline/candidate state is equal before profiling. Counts
are whole-process Callgrind instructions, including setup/teardown; they are
not feed-only native times. Mixed text is approximately 256 KiB, invalid leads
262,185 bytes, and the long dense/error input is one 49,156-byte pattern. Input
sizes and patterns are explicit rather than being relabeled as the complete
native benchmark corpora.

| Input | Baseline whole-process instructions | Candidate instructions | Difference |
| --- | ---: | ---: | ---: |
| Invalid leads |132,589,505|132,603,929|+14,424 (+0.011%)|
| Mixed Unicode |28,923,435|28,928,318|+4,883 (+0.017%)|
| Long dense prefix before error |2,530,149|2,874,369|+344,220 (+13.605%)|

For the dense prefix, the new helper contributes 344,099 exclusive instructions
after aggregating its inlined source locations: approximately seven per valid
prefix byte. It accounts for 99.965% of the whole-process increase. The existing
UTF-8 printer executes exactly 1,283,020 instructions in both versions, and
feed_inner exactly 43,444. This directly attributes nearly all diagnostic extra
work to the added scan, consistent with the native dense-prefix regression.

The invalid-lead helper is never called. Its feed_inner instruction count
changes 25,194,153→25,208,456; mixed feed_inner changes 1,196,896→1,201,658.
The validation wrapper counts are unchanged. These tiny instruction changes
do not explain the observed 7.6–8.9% native invalid-lead timing difference.
Native disassembly shows feed_inner grows 0xfc6→0xfed bytes, reorders some
ground-state branches/loads and moves loop bookkeeping; the print functions
retain identical sizes (UTF-8: 0x1804, ASCII: 0xa74). Code layout, scheduling or
microarchitectural effects remain hypotheses. No hardware-counter experiment
or causal attribution of that native timing difference is claimed.

All raw Callgrind profiles, complete exclusive/inclusive annotations, process
outputs, commands and native disassemblies are retained under profiles/.
attribution.json aggregates only the named function groups, including inlined
source locations; raw profiles remain authoritative. The raw workspace is
`/home/callen/work/hotaru-error-prefix-profile-20261028` locally and on the
workstation. Generated input files and executable outputs are excluded from
Git. input-manifest.json and tools/profile.py preserve reconstruction rules.
