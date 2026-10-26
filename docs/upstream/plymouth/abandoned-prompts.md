# Plymouth: questions left after whoever asked has gone

| | |
| --- | --- |
| Sekka's copy | [`abandoned-prompts.patch`](../../../installer/genkan/rust/system/boot-splash/plymouth/abandoned-prompts.patch), for 26.134.222 |
| Upstream | Not sent. Plymouth's `main` fixes the same bug with f3461296 and e7fb0732 ([plymouth#126](https://gitlab.freedesktop.org/plymouth/plymouth/-/issues/126)), in no release yet |

## What goes wrong

A program asks plymouthd for a passphrase or an answer over its socket, and
plymouthd shows the prompt until it gets one. If that program hangs up first,
26.134.222's `ply_boot_connection_on_hangup()` only marks the connection as
disconnected. The request stays queued and its prompt stays on the splash until
someone types an answer, which then goes nowhere.

systemd's password agent does hang up: it gives up a request once the disk it
was for has been opened another way.

## What it did in Sekka

After a mistyped passphrase in `encrypted-boot-splash`, systemd's agent gave up
the request for a disk that then got its passphrase another way, and the splash
went on asking for a disk that was already open.

## Sekka's patch

Each connection keeps a list of its unanswered requests. When the connection
hangs up, plymouthd drops them from the queue and updates the display. It
changes `src/main.c`, `src/ply-boot-server.c` and `src/ply-boot-server.h`.

## Upstream's fix

- f3461296, "ply-boot-server: cancel pending prompts when requesting client
  disconnects" (2026-05-02). `ply_boot_server_t` gets a
  `connection_hangup_handler`, called from `ply_boot_connection_on_hangup()`.
  The password, question and keystroke requests carry the connection they came
  from, and plymouthd's handler removes that connection's triggers and dismisses
  their prompts.
- e7fb0732, "ply-boot-server: also cancel prompts on partial-request teardown"
  (2026-07-17). The handler also runs when a connection is torn down part way
  through a request.

Both came after 26.134.222, and no release has them yet.

## Why it wasn't sent

Upstream had already fixed it, in its own way. `encrypted-boot-splash` has a
step for exactly this, "a question goes when whoever asked it does": a passphrase
request from a service that is then stopped must leave the splash. It passed on
2026-10-24 with Plymouth's `main` and none of Sekka's patches, as did the steps
after a wrong passphrase ([Testing](README.md#testing)). So upstream's fix covers
what Sekka found.

## When Sekka's copy goes

Once Nixpkgs' Plymouth is a release with f3461296 and e7fb0732, remove
`abandoned-prompts.patch` from `boot-splash/default.nix` and run
`encrypted-boot-splash`. Both changes rework the same functions, so the patch
probably won't apply to that release. Drop it then rather than port it.
