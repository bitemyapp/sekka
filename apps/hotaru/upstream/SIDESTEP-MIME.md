# Prepared native clipboard MIME bundles

Apply `sidestep-mime.patch` after `sidestep-clipboard.patch` and
`sidestep-input.patch`. The preceding patches are unchanged.

`PreparedClipboard::new(plain, representations)` prepares standard UTF-8 text
aliases and additional immutable MIME bytes on a worker. HTML-only selections
are supported without a text fallback. Extra MIME names must be distinct,
printable ASCII, at most 255 bytes, and at most 16 types; callers bound the data.
`Cx::set_prepared_clipboard` publishes the shared `Contents` once. No interface
thread copies or traverses payload bytes. Native transfer budgets, deadlines,
closed-receiver handling and cancellation remain the existing clipboard patch's.

The old `PreparedClipboardText` API converts without copying its allocations.
An owned local selection without text does not fall back to a previous foreign
selection when read. Plain aliases share one allocation; extra MIME buffers
retain the caller's Arcs. Aizome Kit exposes the publication API and its headless
platform records every representation for assertions.

Sidestep UI tests check shared allocations, distinct plain/HTML representations,
HTML-only behavior and rejected duplicate, invalid or excessive MIME metadata.
Application and real Wayland tests additionally exercise formatting and pipe
delivery. Evidence for a tested build is recorded separately with source hashes.
