# Native clipboard encoding boundary

Hotaru's core formatter and native clipboard adapter have different output
contracts. The core formatter returns byte vectors for plain, VT and HTML
representations. In particular, VT copy can retain raw OSC 8 hyperlink URI/ID
bytes even when those bytes are not valid UTF-8.

The native adapter currently turns the formatter's plain/VT representation
into a Rust `String` with `String::from_utf8` before preparing the clipboard
bundle. Invalid UTF-8 therefore produces a visible copy error and preserves the
previous clipboard selection. It is not silently replaced, truncated or
published as corrupted text. However, the native **ANSI Text** copy action
cannot currently publish every raw-byte VT stream that the core can format.

This limitation was identified by source review on October 28, 2026 in
`desktop/aizome/apps/hotaru/src/clipboard.rs` (`prepare_copy`). The large native
clipboard smoke exercises valid ASCII plain/HTML representations and does not
certify arbitrary-byte VT transport.

A later policy decision must specify whether ANSI copy rejects invalid UTF-8,
uses a separate byte-oriented MIME representation, or exposes explicitly
documented byte-preserving terminal content. Changing only the conversion to a
lossy one would discard information. Advertising invalid UTF-8 as a UTF-8 text
MIME type would also violate the advertised encoding. Whatever representation
is chosen must retain bounded worker preparation, cancellation, exact MIME
bytes, stale-selection checks and constant-time UI publication.

This note records current behavior; it does not change product code or decide
the future MIME/encoding policy.
