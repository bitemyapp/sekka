# Ghostty's author on the zero-scrollback comparison, 2 November 2026

Mitchell Hashimoto (`@mitchellh` on X), Ghostty's author, posted this reply
on X: <https://x.com/mitchellh/status/2117374562333872319>, at
2026-11-02 21:53:29 UTC (15:53 Central), the time the post's ID encodes.
It is kept here as the source for what
[terminals.md](../../../terminals.md) and aizome.computer/performance say
he pointed out, in case the post becomes unreachable.

He reran the earlier library-level comparison,
[ghostty-comparison-2026-10-30.md](../../../../../apps/hotaru/docs/ghostty-comparison-2026-10-30.md),
with its harness (`hotaru-replay`, the C adapter for libghostty-vt, the
oracle). His figures are for Ghostty main `91973f2b2` against Hotaru at
Sekka `59786be0`, fed bytes in Hotaru's feed-only mode, which keeps no
history. They are not the whole-application measurements in terminals.md,
and nothing here was rerun by us.

What came of it: the throughput floods in terminals.md were measured again
with history in every terminal
([Throughput with history](../../../terminals.md#throughput-with-history)).
Hotaru's slowdown as its history grows is the subject of PR #261.

## Files

Kept at the resolution they were saved at, unedited:

- `post-1.webp` (1268 × 1756) and `post-2.webp` (1278 × 1582):
  screenshots of the post in X, saved by Sekka's maintainer on
  5 November 2026.
- The three graphs attached to the post, as X serves them (2000 px wide):
  - `graph-hotaru-corpora.webp`: Ghostty against Hotaru, Hotaru's datasets
    and runner, at zero, 10 KB and 50 MB of scrollback (Hotaru = 1×,
    higher is better for Ghostty).
  - `graph-ghostty-gen-corpora.webp`: the same with `ghostty-gen`'s
    datasets.
  - `graph-hotaru-history-slowdown.webp`: Hotaru's runtime with 1,000 and
    50,000 lines of history, against none.

## The post's text

As copied from X:

> Alright! I found time this weekend and today to reproduce these results. I used your harness completely (your replay tool, C adapter for libghostty, oracle, etc.). Thank you for providing all of it.
>
> The headline is I was able to reproduce most of your results, with one major asterisk: your harness configures libghostty-vt explicitly in zero-scrollback mode, which eliminates a lot of the optimizations we make. It is a fair comparison for alt screen, but not fair for "normal" terminal usage, I'd say.
>
> To cover the gap, I modified your harness to run with zero scrollback (yours), libghostty-vt defaults (10KB), and Ghostty GUI's defaults (50MB). I attached those results. With any scrollback, libghostty is significantly faster.
>
> I also noticed Hotaru gets continuously slower as scrollback increases, which causes terrible benchmark skew. I suspect you have an O(N) somewhere screwing you. Since Hotaru at zero scrollback had the best results, my graphs show that. Ghostty did not exhibit the scrollback slowdown problem.
>
> I spent my nights over the weekend closing the gap for the styled, hyperlink, and cursor/erase stuff, and I rebenchmarked main vs main just an hour ago. So this includes that. I want to be clear there WAS a gap here, and I fixed it.
>
> I really appreciate the great harness and competition. I hope you can now mog these results and we can continue!
>
> I attached three graphs.
>
> The first two are throughput numbers. One is your data set plus your runner. The other is `ghostty-gen` data plus your runner. I include `ghostty-gen` because it produces more random data with more knobs than yours. Its all in our repo, so you can reproduce (I'll show you mine if you show me yours kind of thing lol).
>
> The last shows your slowdown on your own input data. I didn't include Ghostty here at all since I can't get apples-to-apples timings here because your harness for this includes command startup and so on. Ghostty doesn't exhibit any throughout number changes though for any increase in history size, its constant.
>
> Feel free to reproduce, I look forward to hearing any feedback, and enjoy the high quality competition. I'm still working on more improvements, so just include the SHA of Ghostty if you benchmark again.
