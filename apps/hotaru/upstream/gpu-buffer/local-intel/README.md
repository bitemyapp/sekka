# Frozen local Intel capability evidence

The unchanged `apps/hotaru/tools/gpu-buffer-probe.c` SHA256 9ae5753f7a6d316607009c127e0e991f6c0919772328e912c523c2c2abcf4102 passed in a task-owned rooted Nix store/private user and mount namespace. No privileged host change occurred. This folder contains compact reports, the separate bounded local runner, the exact namespace/environment launch scripts, and source-numbered syscall excerpts.

Full source, executable, JSONL and 172,384-byte syscall trace remain at `/home/callen/work/hotaru-local-gui-20261028/gpu-capability/`. Their SHA256 identities are recorded in `qualification.json`. The scripts retain the exact task paths as provenance; they are not a portable installed application interface. The generic probe source itself is unchanged.

The copied runtime packages are the historical qualified Hotaru e2b1ff7 package `/nix/store/26jmmbqz0m6j903y1vm3c1d9si3b7j41-hotaru-0.1.0` and Ghostty6ba144158577d033ccb6ed4b574e4013957c9161 package. Package portability records do not qualify newer integrated ASCII/pixel-scroll code. The local GUI pilot was rejected for CPU saturation before app launch; it supplies no accepted timing.

This evidence proves one small offscreen, CPU-waited transfer between unshared contexts. It does not establish asynchronous cross-process synchronization, Wayland import/presentation, a native GPU renderer or performance parity.
