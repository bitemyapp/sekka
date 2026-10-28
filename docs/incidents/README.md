# Incident records

One record per reported failure: what was seen, the evidence, the cause as
far as it was established, the fix, and what the fix's checks do and don't
cover. [DEVELOPERS.md](../../DEVELOPERS.md) describes how things work now and
links here for the history behind them.

Name records `YYYY-MM-DD-short-name.md`, dated by the report. Keep the
report's evidence apart from conclusions drawn from it, and say when a cause
is plausible rather than proven. Name the pull request or commit that fixed
it, the regression checks and their negative controls, and what stayed
unverified (physical hardware, the release image). Leave private data from
logs out.

| Reported | Incident | Component | Fix |
| --- | --- | --- | --- |
| 2026-10-27 | [Lock screen black after a display change](2026-10-27-lock-screen-display-change.md) | Aizome lock screen | [#126](https://github.com/bitemyapp/sekka/pull/126) |
