# Private Intel Wayland DMA-BUF transport

The final isolated diagnostic passed after independent lifecycle review. It runs
Sway 1.12/wlroots 0.20.2 with Intel PTL GLES and Mesa 26.2.5 in the task-owned
rooted Nix store and unprivileged namespace. Only the existing render node is
exposed. It does not connect to the current desktop or change host settings.

The exported AB24/linear pair matches an advertised pair. Asynchronous import
succeeds before the duplicate transport FD closes. The source remains alive and
immutable through commit, capture, detach, and `wl_buffer.release`. All 3,072 RGB
bytes of the central 32×32 capture match. The XB24 capture padding is undefined
and is not counted as alpha. The original unshared EGL consumer subsequently
checks all 4,096 RGBA bytes.

Flags are zero: producer readback row zero maps to the surface top. The failed
Y_INVERT attempt is retained. Future GL scene code must use a matching export
projection. Frame callback timestamps have an unspecified millisecond epoch;
presentation reports CLOCK_MONOTONIC, flags zero and refresh zero on the private
headless output. This does not establish physical scanout or performance.

The bounded pump rechecks its absolute deadline even while pending events recur.
The runner uses the frozen matrix helper (`889c1151…`) for process-local
subreaper/pidfd descendant cleanup. The final run required no forced cleanup and
all owned processes exited. Independent review found no further concrete issue.

`result.json` records exact source/protocol/compiler command, executable hash,
loaded paths, events and runtime environment. `qualification.json` freezes raw
artifact hashes. `producer.c` is generated from the unchanged offscreen probe
with one typed callback after its fence; the two C files are diagnostic code,
not a production dependency. `attempts.json` preserves setup/import failures and
the pre-review success. Raw logs, captures and generated protocol source remain
at `/home/callen/work/hotaru-local-gui-20261028/gpu-transport`.

Reproduction uses that directory layout, the parent `run-private-gpu.sh`, the
exact frozen `tools/frame-matrix.py` cleanup helper and package closure documented
in `../local-intel`. The runner generates protocol code from the recorded cached
XMLs. These archived scripts preserve the executed paths; they are not a portable
installer. The captured PNG is a direct RGB decoding of the saved XB24 bytes.

No Rust GPU renderer, asynchronous GPU handoff or native app integration is
claimed by this transport diagnostic.
