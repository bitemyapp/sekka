//! Kitty's clipboard protocol (OSC 5522): request decoding, write
//! transactions, session password grants and response encoding.
//!
//! Ported from pinned Ghostty's MIT-licensed `src/terminal/kitty/clipboard*.zig`,
//! which follows kitty's reference implementation (`kitty/clipboard.py`) where
//! it and the prose specification disagree:
//!
//! * Malformed metadata (a record without `=`, including empty metadata) and
//!   an unknown or missing `type` drop the packet without a response.
//! * Base64 (payloads, and the `mime`, `name` and `pw` values) is strict RFC
//!   4648 with required padding. An invalid value on `wdata` or `walias` aborts
//!   an in-flight write with `EINVAL`; an invalid `read` is dropped.
//! * Decoded metadata and MIME lists must be UTF-8.
//! * One MIME type's `wdata` payloads form a single base64 stream split at
//!   any boundary; a packet ending exactly at padding restarts the stream.
//! * `id` keeps only `[a-zA-Z0-9-_+.]`, at most 512 bytes, and is echoed in
//!   every response packet (left out when empty).
//! * A `type=write` replaces any write in flight; a commit (`wdata` without a
//!   MIME type) with none in flight is ignored.
//!
//! The terminal reports raw packets as [`crate::TerminalEvent::KittyClipboard`];
//! whoever owns the PTY keeps a [`WriteState`] and [`Grants`] per terminal and
//! answers with [`Response`], [`ReadSuccess`] and [`PasteEvent`].
//! Specification: <https://sw.kovidgoyal.net/kitty/clipboard/>

use std::sync::Arc;

/// Kitty's own id limit.
pub const MAX_ID_LEN: usize = 512;
/// Decoded password limit; longer ones can never match a grant.
pub const MAX_PW_LEN: usize = 128;
/// Decoded MIME type limit; longer ones drop the packet.
pub const MAX_MIME_LEN: usize = 256;
/// Decoded name limit; names are shown in permission prompts.
pub const MAX_NAME_LEN: usize = 256;
/// Ghostty's default limit on one write transaction's decoded bytes.
pub const MAX_WRITE_SIZE: usize = 64 * 1024 * 1024;
pub const MAX_WRITE_MIMES: usize = 64;
pub const MAX_WRITE_ALIASES: usize = 64;
/// Raw bytes per DATA packet in read responses, as the protocol specifies.
pub const READ_CHUNK_SIZE: usize = 4096;
/// Requested types one read serves; extras are never served, which is how
/// the protocol reports an unavailable type anyway.
pub const MAX_READ_MIMES: usize = 4;
/// Types a paste event's listing reports.
pub const MAX_LISTING_MIMES: usize = 16;
/// The type that asks for the listing of available types.
pub const TARGETS_MIME: &str = ".";
/// One-time password length and alphabet, as kitty's (alphanumeric without
/// easily confused characters).
pub const OTP_LEN: usize = 22;
pub const OTP_ALPHABET: &[u8] = b"23456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";

/// The text types every text representation answers for.
pub const TEXT_MIMES: [&str; 5] = [
    "text/plain",
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];

pub fn is_text_mime(mime: &str) -> bool {
    TEXT_MIMES.contains(&mime)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Read,
    Walias,
    Wdata,
    Write,
}

impl Operation {
    fn parse(value: &[u8]) -> Option<Self> {
        Some(match value {
            b"read" => Self::Read,
            b"walias" => Self::Walias,
            b"wdata" => Self::Wdata,
            b"write" => Self::Write,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Walias => "walias",
            Self::Wdata => "wdata",
            Self::Write => "write",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Data,
    Done,
    Ebusy,
    Efbig,
    Einval,
    Eio,
    Enosys,
    Eperm,
    Ok,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Data => "DATA",
            Self::Done => "DONE",
            Self::Ebusy => "EBUSY",
            Self::Efbig => "EFBIG",
            Self::Einval => "EINVAL",
            Self::Eio => "EIO",
            Self::Enosys => "ENOSYS",
            Self::Eperm => "EPERM",
            Self::Ok => "OK",
        }
    }
}

/// Which clipboard: `loc=primary`, or the clipboard for anything else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Location {
    #[default]
    Standard,
    Primary,
}

/// How the request ended, which its responses repeat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Terminator {
    Bel,
    #[default]
    St,
}

impl Terminator {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::Bel => b"\x07",
            Self::St => b"\x1b\\",
        }
    }
}

/// A decoded text value was not UTF-8, not base64, or past its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidValue;

/// The decoded, validated metadata of one packet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub op: Option<Operation>,
    pub loc: Location,
    /// Sanitized; empty means none.
    pub id: String,
    /// Empty means absent: a `wdata` with none commits.
    pub mime: String,
    /// A password counts only with a name, as the specification says.
    pub pw: String,
    /// The program's own name for itself, shown in prompts.
    pub name: String,
}

impl Metadata {
    /// `Ok(None)` drops the packet silently; `Err` means a decoded value was
    /// invalid, which aborts a write in flight when the packet is `wdata` or
    /// `walias` (see [`Metadata::operation`]).
    pub fn parse(raw: &[u8]) -> Result<Option<Metadata>, InvalidValue> {
        let Some(fields) = RawFields::parse(raw) else {
            return Ok(None);
        };
        let Some(op) = fields.op.and_then(Operation::parse) else {
            return Ok(None);
        };
        let mime = decode_value(fields.mime, MAX_MIME_LEN).map_err(|_| InvalidValue)?;
        // An over-long password can never match a grant: it's as if absent.
        let pw = match decode_value(fields.pw, MAX_PW_LEN) {
            Ok(pw) => pw,
            Err(ValueError::Overflow) => String::new(),
            Err(ValueError::Invalid) => return Err(InvalidValue),
        };
        let name = decode_value(fields.name, MAX_NAME_LEN).map_err(|_| InvalidValue)?;
        Ok(Some(Metadata {
            op: Some(op),
            loc: if fields.loc == b"primary" {
                Location::Primary
            } else {
                Location::Standard
            },
            id: sanitize_id(fields.id),
            mime,
            pw,
            name,
        }))
    }

    /// The operation of syntactically valid metadata, without decoding values.
    pub fn operation(raw: &[u8]) -> Option<Operation> {
        RawFields::parse(raw)?.op.and_then(Operation::parse)
    }

    /// The password the request may use: none without a name.
    pub fn effective_pw(&self) -> &str {
        if self.name.is_empty() { "" } else { &self.pw }
    }
}

#[derive(Default)]
struct RawFields<'a> {
    op: Option<&'a [u8]>,
    loc: &'a [u8],
    id: &'a [u8],
    mime: &'a [u8],
    pw: &'a [u8],
    name: &'a [u8],
}

impl<'a> RawFields<'a> {
    fn parse(raw: &'a [u8]) -> Option<Self> {
        let mut fields = Self::default();
        // Every record is checked, and the last duplicate wins.
        for record in raw.split(|&b| b == b':') {
            let equal = record.iter().position(|&b| b == b'=')?;
            let (key, value) = (&record[..equal], &record[equal + 1..]);
            match key {
                b"type" => fields.op = Some(value),
                b"loc" => fields.loc = value,
                b"id" => fields.id = value,
                b"mime" => fields.mime = value,
                b"pw" => fields.pw = value,
                b"name" => fields.name = value,
                _ => {}
            }
        }
        Some(fields)
    }
}

fn sanitize_id(value: &[u8]) -> String {
    value
        .iter()
        .filter(|b| b.is_ascii_alphanumeric() || b"-_+.".contains(b))
        .take(MAX_ID_LEN)
        .map(|&b| b as char)
        .collect()
}

enum ValueError {
    Overflow,
    Invalid,
}

fn decode_value(value: &[u8], max: usize) -> Result<String, ValueError> {
    // Avoid decoding hostile large values.
    if value.len() > max.div_ceil(3) * 4 {
        return Err(ValueError::Overflow);
    }
    let decoded = base64::decode_strict(value).ok_or(ValueError::Invalid)?;
    let text = String::from_utf8(decoded).map_err(|_| ValueError::Invalid)?;
    if text.len() > max {
        return Err(ValueError::Overflow);
    }
    Ok(text)
}

/// A strictly decoded payload (a read's MIME list, a `walias` alias list).
pub fn decode_payload(payload: &[u8]) -> Option<Vec<u8>> {
    base64::decode_strict(payload)
}

/// A decoded list's whitespace-separated types, as Python's `str.split()`.
pub fn mime_list(text: &str) -> impl Iterator<Item = &str> {
    text.split_ascii_whitespace()
}

/// A read asking only for the listing (no data types) is never prompted.
pub fn read_prompt_exempt(data_mimes: usize) -> bool {
    data_mimes == 0
}

/// Strict RFC 4648 standard-alphabet base64.
pub mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn value(c: u8) -> Option<u8> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }

    pub fn encode(input: &[u8], out: &mut Vec<u8>) {
        out.reserve(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(ALPHABET[(n >> 18) as usize & 63]);
            out.push(ALPHABET[(n >> 12) as usize & 63]);
            out.push(if chunk.len() > 1 {
                ALPHABET[(n >> 6) as usize & 63]
            } else {
                b'='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[n as usize & 63]
            } else {
                b'='
            });
        }
    }

    /// One complete group: padding only as a suffix of at most two, `=` in
    /// the third place requiring it in the fourth. Unused bits are not
    /// checked, as Ghostty's decoder does not.
    fn group(g: &[u8], out: &mut Vec<u8>) -> Option<bool> {
        let pad = match (g[2], g[3]) {
            (b'=', b'=') => 2,
            (b'=', _) => return None,
            (_, b'=') => 1,
            _ => 0,
        };
        let mut n = 0u32;
        for (i, &c) in g.iter().enumerate() {
            n = (n << 6) | u32::from(if i >= 4 - pad { 0 } else { value(c)? });
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
        Some(pad > 0)
    }

    /// The whole input, which must be complete padded groups, padding only
    /// at its end.
    pub fn decode_strict(input: &[u8]) -> Option<Vec<u8>> {
        if !input.len().is_multiple_of(4) {
            return None;
        }
        let mut out = Vec::with_capacity(input.len() / 4 * 3);
        let groups = input.len() / 4;
        for (i, g) in input.chunks(4).enumerate() {
            if group(g, &mut out)? && i + 1 != groups {
                return None;
            }
        }
        Some(out)
    }

    /// A stream that isn't valid base64, or ended mid-group.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Invalid;

    /// One payload split across chunks at any byte boundary. Padding ends a
    /// feed: a padded group followed by more of the same feed is invalid,
    /// and a feed ending at padding restarts the stream, as kitty's does.
    #[derive(Clone, Debug, Default)]
    pub struct Streaming {
        carry: [u8; 4],
        carried: usize,
    }

    impl Streaming {
        /// Most decoded bytes `input` can produce.
        pub fn max_len(&self, input: &[u8]) -> usize {
            (self.carried + input.len()) / 4 * 3
        }

        pub fn feed(&mut self, mut input: &[u8], out: &mut Vec<u8>) -> Result<(), Invalid> {
            if self.carried > 0 {
                let take = (4 - self.carried).min(input.len());
                for (offset, &c) in input[..take].iter().enumerate() {
                    if !partial(self.carried + offset, c) {
                        return Err(Invalid);
                    }
                }
                self.carry[self.carried..self.carried + take].copy_from_slice(&input[..take]);
                self.carried += take;
                input = &input[take..];
                if self.carried < 4 {
                    return Ok(());
                }
                self.carried = 0;
                let carry = self.carry;
                if group(&carry, out).ok_or(Invalid)? {
                    return if input.is_empty() {
                        Ok(())
                    } else {
                        Err(Invalid)
                    };
                }
            }
            let whole = input.len() - input.len() % 4;
            let groups = whole / 4;
            for (i, g) in input[..whole].chunks(4).enumerate() {
                if group(g, out).ok_or(Invalid)? {
                    // Terminal padding must end the feed.
                    return if i + 1 == groups && whole == input.len() {
                        Ok(())
                    } else {
                        Err(Invalid)
                    };
                }
            }
            let tail = &input[whole..];
            for (offset, &c) in tail.iter().enumerate() {
                if !partial(offset, c) {
                    return Err(Invalid);
                }
            }
            self.carry[..tail.len()].copy_from_slice(tail);
            self.carried = tail.len();
            Ok(())
        }

        /// The stream must have ended on a complete group. Ready for a fresh
        /// stream either way.
        pub fn finish(&mut self) -> Result<(), Invalid> {
            let carried = std::mem::take(&mut self.carried);
            if carried == 0 { Ok(()) } else { Err(Invalid) }
        }
    }

    /// Valid in a group still waiting for the rest: only the last two
    /// places may open the padding.
    fn partial(position: usize, c: u8) -> bool {
        value(c).is_some() || (position >= 2 && c == b'=')
    }
}

/// Why a write transaction failed; the caller answers `EFBIG` or `EINVAL`
/// and drops it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    TooLarge,
    Invalid,
}

#[derive(Clone, Debug)]
struct Entry {
    mime: String,
    start: usize,
    len: usize,
}

/// One `type=write` with its `wdata` chunks and `walias` aliases, until the
/// commit or an error.
#[derive(Clone, Debug)]
pub struct WriteState {
    pub loc: Location,
    pub id: String,
    pub pw: String,
    pub name: String,
    spool: Vec<u8>,
    entries: Vec<Entry>,
    aliases: Vec<(String, String)>,
    max_size: usize,
    current: Option<usize>,
    decoder: base64::Streaming,
}

/// A committed write: the program's types in arrival order, then aliases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committed {
    pub loc: Location,
    pub id: String,
    pub pw: String,
    pub name: String,
    pub contents: Vec<(String, Arc<[u8]>)>,
}

impl WriteState {
    /// Begin from a `type=write`, accumulating at most `max_size` bytes.
    pub fn new(meta: &Metadata, max_size: usize) -> Self {
        Self {
            loc: meta.loc,
            id: meta.id.clone(),
            pw: meta.pw.clone(),
            name: meta.name.clone(),
            spool: Vec::new(),
            entries: Vec::new(),
            aliases: Vec::new(),
            max_size,
            current: None,
            decoder: base64::Streaming::default(),
        }
    }

    /// One `wdata` chunk for `meta.mime`, which must not be empty (an empty
    /// one commits).
    pub fn data(&mut self, meta: &Metadata, payload: &[u8]) -> Result<(), WriteError> {
        debug_assert!(!meta.mime.is_empty());
        let switching = self
            .current
            .is_none_or(|i| self.entries[i].mime != meta.mime);
        if switching {
            if let Some(i) = self.current {
                // The previous type's stream must have ended on a group.
                self.decoder
                    .finish()
                    .map_err(|base64::Invalid| WriteError::Invalid)?;
                self.entries[i].len = self.spool.len() - self.entries[i].start;
            }
            // A type used again starts over.
            if let Some(i) = self.entries.iter().position(|e| e.mime == meta.mime) {
                self.entries[i].start = self.spool.len();
                self.entries[i].len = 0;
                self.current = Some(i);
            } else if self.entries.len() >= MAX_WRITE_MIMES {
                self.current = None;
                return Ok(());
            } else {
                self.entries.push(Entry {
                    mime: meta.mime.clone(),
                    start: self.spool.len(),
                    len: 0,
                });
                self.current = Some(self.entries.len() - 1);
            }
        }
        // One payload is bounded by the OSC capture, so decoding it before
        // the check costs at most that. Corrupt data must not be dropped
        // quietly, and partial contents must never reach the clipboard:
        // either aborts the whole write.
        self.spool.reserve(self.decoder.max_len(payload));
        self.decoder
            .feed(payload, &mut self.spool)
            .map_err(|base64::Invalid| WriteError::Invalid)?;
        if self.spool.len() > self.max_size {
            return Err(WriteError::TooLarge);
        }
        Ok(())
    }

    /// One `walias`: `meta.mime` is the target with data, the payload the
    /// aliases. A repeated alias takes its newest target.
    pub fn alias(&mut self, meta: &Metadata, payload: &[u8]) -> Result<(), WriteError> {
        debug_assert!(!meta.mime.is_empty());
        let decoded = decode_payload(payload).ok_or(WriteError::Invalid)?;
        let text = std::str::from_utf8(&decoded).map_err(|_| WriteError::Invalid)?;
        for name in mime_list(text) {
            if name.len() > MAX_MIME_LEN {
                continue;
            }
            if let Some(alias) = self.aliases.iter_mut().find(|(alias, _)| alias == name) {
                alias.1.clone_from(&meta.mime);
            } else if self.aliases.len() >= MAX_WRITE_ALIASES {
                return Ok(());
            } else {
                self.aliases.push((name.to_owned(), meta.mime.clone()));
            }
        }
        Ok(())
    }

    /// The commit (`wdata` without a type). Aliases apply in order against
    /// the growing map, so chained aliases work as kitty's; one without a
    /// target is dropped.
    pub fn commit(mut self) -> Result<Committed, WriteError> {
        if let Some(i) = self.current.take() {
            self.decoder
                .finish()
                .map_err(|base64::Invalid| WriteError::Invalid)?;
            self.entries[i].len = self.spool.len() - self.entries[i].start;
        }
        let mut contents: Vec<(String, Arc<[u8]>)> = self
            .entries
            .iter()
            .map(|e| {
                (
                    e.mime.clone(),
                    Arc::from(&self.spool[e.start..e.start + e.len]),
                )
            })
            .collect();
        for (alias, target) in &self.aliases {
            let Some(data) = contents
                .iter()
                .find(|(mime, _)| mime == target)
                .map(|(_, data)| data.clone())
            else {
                continue;
            };
            match contents.iter_mut().find(|(mime, _)| mime == alias) {
                Some(existing) => existing.1 = data,
                None => contents.push((alias.clone(), data)),
            }
        }
        Ok(Committed {
            loc: self.loc,
            id: self.id,
            pw: self.pw,
            name: self.name,
            contents,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Read,
    Write,
}

#[derive(Clone, Debug)]
struct Grant {
    pw: String,
    read: bool,
    write: bool,
    one_time: bool,
}

/// Session password grants: requests carrying one skip the prompt. Paste
/// events record one-time read grants.
#[derive(Clone, Debug, Default)]
pub struct Grants {
    entries: Vec<Grant>,
}

impl Grants {
    const MAX: usize = 32;

    /// A grant for `pw`; one already held gains the direction.
    pub fn grant(&mut self, pw: &str, direction: Direction, one_time: bool) {
        if pw.is_empty() || pw.len() > MAX_PW_LEN {
            return;
        }
        let index = match self.entries.iter().position(|g| g.pw == pw) {
            Some(i) => {
                self.entries[i].one_time &= one_time;
                i
            }
            None => {
                if self.entries.len() >= Self::MAX {
                    self.entries.remove(0);
                }
                self.entries.push(Grant {
                    pw: pw.to_owned(),
                    read: false,
                    write: false,
                    one_time,
                });
                self.entries.len() - 1
            }
        };
        match direction {
            Direction::Read => self.entries[index].read = true,
            Direction::Write => self.entries[index].write = true,
        }
    }

    /// Whether `pw` grants `direction`. A one-time grant goes with the
    /// check, whatever its direction, as kitty pops it.
    pub fn use_grant(&mut self, pw: &str, direction: Direction) -> bool {
        if pw.is_empty() {
            return false;
        }
        let Some(i) = self.entries.iter().position(|g| g.pw == pw) else {
            return false;
        };
        let allowed = match direction {
            Direction::Read => self.entries[i].read,
            Direction::Write => self.entries[i].write,
        };
        if self.entries[i].one_time {
            self.entries.swap_remove(i);
        }
        allowed
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// A one-time password from `random` bytes (rejection-sampled into the
/// alphabet), or none when they run out first: never a weak password.
pub fn one_time_password(random: &[u8]) -> Option<String> {
    let limit = 256 / OTP_ALPHABET.len() * OTP_ALPHABET.len();
    let password: String = random
        .iter()
        .filter(|&&b| usize::from(b) < limit)
        .map(|&b| OTP_ALPHABET[usize::from(b) % OTP_ALPHABET.len()] as char)
        .take(OTP_LEN)
        .collect();
    (password.len() == OTP_LEN).then_some(password)
}

/// One response packet. An empty payload sends no payload section.
#[derive(Clone, Copy, Debug)]
pub struct Response<'a> {
    pub op: Operation,
    pub status: Status,
    pub primary: bool,
    pub id: &'a str,
    pub mime: Option<&'a str>,
    pub pw: Option<&'a str>,
    pub payload: &'a [u8],
    pub terminator: Terminator,
}

impl<'a> Response<'a> {
    pub fn status(op: Operation, status: Status, id: &'a str, terminator: Terminator) -> Self {
        Self {
            op,
            status,
            primary: false,
            id,
            mime: None,
            pw: None,
            payload: b"",
            terminator,
        }
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        self.encode_metadata(out);
        if !self.payload.is_empty() {
            out.push(b';');
            base64::encode(self.payload, out);
        }
        out.extend_from_slice(self.terminator.bytes());
    }

    /// Everything before the payload section, in kitty's field order.
    fn encode_metadata(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b]5522;type=");
        out.extend_from_slice(self.op.name().as_bytes());
        out.extend_from_slice(b":status=");
        out.extend_from_slice(self.status.name().as_bytes());
        if self.primary {
            out.extend_from_slice(b":loc=primary");
        }
        if !self.id.is_empty() {
            out.extend_from_slice(b":id=");
            out.extend_from_slice(self.id.as_bytes());
        }
        if let Some(mime) = self.mime {
            out.extend_from_slice(b":mime=");
            base64::encode(mime.as_bytes(), out);
        }
        if let Some(pw) = self.pw {
            out.extend_from_slice(b":pw=");
            base64::encode(pw.as_bytes(), out);
        }
    }
}

/// A whole successful read: OK, the listing if asked for, DATA chunks for
/// each representation served, and DONE. An empty representation sends no
/// DATA, which is how the protocol says a type is unavailable.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadSuccess<'a> {
    pub primary: bool,
    pub id: &'a str,
    /// A paste event's one-time password, in every packet.
    pub pw: Option<&'a str>,
    pub list: bool,
    pub available: &'a [String],
    pub contents: &'a [(String, Arc<[u8]>)],
    pub terminator: Terminator,
}

impl ReadSuccess<'_> {
    pub fn encode(&self, out: &mut Vec<u8>) {
        let packet = |status| Response {
            op: Operation::Read,
            status,
            primary: false,
            id: self.id,
            mime: None,
            pw: self.pw,
            payload: b"",
            terminator: self.terminator,
        };
        Response {
            primary: self.primary,
            ..packet(Status::Ok)
        }
        .encode(out);
        if self.list {
            let listing = Response {
                mime: Some(TARGETS_MIME),
                ..packet(Status::Data)
            };
            listing.encode_metadata(out);
            if !self.available.is_empty() {
                // One DATA packet's raw bound; types that don't fit go.
                let mut raw = Vec::new();
                for (i, mime) in self.available.iter().enumerate() {
                    let separator = usize::from(i > 0);
                    if raw.len() + separator + mime.len() + 1 > READ_CHUNK_SIZE {
                        break;
                    }
                    if i > 0 {
                        raw.push(b' ');
                    }
                    raw.extend_from_slice(mime.as_bytes());
                }
                raw.push(b'\n');
                out.push(b';');
                base64::encode(&raw, out);
            }
            out.extend_from_slice(self.terminator.bytes());
        }
        for (mime, data) in self.contents {
            for chunk in data.chunks(READ_CHUNK_SIZE) {
                Response {
                    mime: Some(mime),
                    payload: chunk,
                    ..packet(Status::Data)
                }
                .encode(out);
            }
        }
        packet(Status::Done).encode(out);
    }
}

/// An unsolicited paste event (mode 5522): a read response listing the
/// clipboard's types, with the one-time password the program reads with.
#[derive(Clone, Copy, Debug)]
pub struct PasteEvent<'a> {
    pub primary: bool,
    pub pw: &'a str,
    pub available: &'a [String],
    pub terminator: Terminator,
}

impl PasteEvent<'_> {
    pub fn encode(&self, out: &mut Vec<u8>) {
        let available = &self.available[..self.available.len().min(MAX_LISTING_MIMES)];
        ReadSuccess {
            primary: self.primary,
            pw: Some(self.pw),
            list: true,
            available,
            terminator: self.terminator,
            ..ReadSuccess::default()
        }
        .encode(out);
    }
}

/// A program's whole read, for whoever owns the clipboard to answer with
/// [`Read::success`] or [`Read::status`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub loc: Location,
    /// The data types asked for, in order, at most [`MAX_READ_MIMES`].
    pub mimes: Vec<String>,
    /// The listing of available types (`.`) was asked for too.
    pub list: bool,
    pub id: String,
    /// The password that counts: none without a name.
    pub pw: String,
    pub name: String,
    /// A session grant covers it, so nobody needs asking.
    pub granted: bool,
    pub terminator: Terminator,
}

impl Read {
    /// Only the listing: never asked about, since no data goes.
    pub fn prompt_exempt(&self) -> bool {
        read_prompt_exempt(self.mimes.len())
    }

    pub fn status(&self, status: Status, out: &mut Vec<u8>) {
        Response::status(Operation::Read, status, &self.id, self.terminator).encode(out);
    }

    /// `contents` under the names the program asked for, in its order.
    pub fn success(
        &self,
        available: &[String],
        contents: &[(String, Arc<[u8]>)],
        out: &mut Vec<u8>,
    ) {
        ReadSuccess {
            primary: self.loc == Location::Primary,
            id: &self.id,
            pw: None,
            list: self.list,
            available,
            contents,
            terminator: self.terminator,
        }
        .encode(out);
    }
}

/// A program's committed write, answered with [`Write::status`] (`DONE`
/// once it is on the clipboard).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub loc: Location,
    pub id: String,
    /// The password that counts: none without a name.
    pub pw: String,
    pub name: String,
    /// The types in arrival order, then aliases; none clears the clipboard.
    pub contents: Vec<(String, Arc<[u8]>)>,
    pub granted: bool,
    pub terminator: Terminator,
}

impl Write {
    pub fn status(&self, status: Status, out: &mut Vec<u8>) {
        Response::status(Operation::Write, status, &self.id, self.terminator).encode(out);
    }

    /// The bytes it holds, counting data aliases share once.
    pub fn size(&self) -> usize {
        let mut seen: Vec<*const u8> = Vec::new();
        self.contents
            .iter()
            .filter(|(_, data)| {
                let first = !seen.contains(&data.as_ptr());
                seen.push(data.as_ptr());
                first
            })
            .map(|(_, data)| data.len())
            .sum()
    }
}

/// What a packet asks of whoever owns the clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Read(Box<Read>),
    Write(Box<Write>),
}

impl Request {
    /// The answer for a request nobody can take now: try again later.
    pub fn busy(&self, out: &mut Vec<u8>) {
        match self {
            Self::Read(read) => read.status(Status::Ebusy, out),
            Self::Write(write) => write.status(Status::Ebusy, out),
        }
    }
}

/// One terminal's protocol state: the write in flight, the session's
/// password grants and the write limit. The terminal answers malformed and
/// oversized writes itself, as Ghostty's stream handler does, and reports
/// whole reads and committed writes.
#[derive(Clone, Debug)]
pub(crate) struct Protocol {
    write: Option<WriteState>,
    pub(crate) grants: Grants,
    pub(crate) write_limit: usize,
}

impl Default for Protocol {
    fn default() -> Self {
        Self {
            write: None,
            grants: Grants::default(),
            write_limit: MAX_WRITE_SIZE,
        }
    }
}

impl Protocol {
    /// One OSC 5522 body split at its first `;`. Replies go to `replies`.
    pub(crate) fn packet(
        &mut self,
        metadata: &[u8],
        payload: Option<&[u8]>,
        terminator: Terminator,
        replies: &mut Vec<u8>,
    ) -> Option<Request> {
        let meta = match Metadata::parse(metadata) {
            Ok(meta) => meta?,
            Err(InvalidValue) => {
                if matches!(
                    Metadata::operation(metadata),
                    Some(Operation::Wdata | Operation::Walias)
                ) {
                    self.fail(Status::Einval, terminator, replies);
                }
                return None;
            }
        };
        let payload = payload.unwrap_or_default();
        match meta.op? {
            Operation::Read => self.read(&meta, payload, terminator),
            Operation::Write => {
                // A new write replaces any in flight, without an answer.
                self.write = Some(WriteState::new(&meta, self.write_limit));
                None
            }
            Operation::Wdata => {
                // Data without a write is ignored, as kitty does.
                let state = self.write.as_mut()?;
                if meta.mime.is_empty() {
                    return self.commit(terminator, replies);
                }
                match state.data(&meta, payload) {
                    Ok(()) => {}
                    Err(WriteError::TooLarge) => self.fail(Status::Efbig, terminator, replies),
                    Err(WriteError::Invalid) => self.fail(Status::Einval, terminator, replies),
                }
                None
            }
            Operation::Walias => {
                let state = self.write.as_mut()?;
                if meta.mime.is_empty() || state.alias(&meta, payload).is_err() {
                    self.fail(Status::Einval, terminator, replies);
                }
                None
            }
        }
    }

    fn read(&mut self, meta: &Metadata, payload: &[u8], terminator: Terminator) -> Option<Request> {
        // An undecodable list drops the read without an answer, as kitty.
        let decoded = decode_payload(payload)?;
        let text = std::str::from_utf8(&decoded).ok()?;
        let (mut mimes, mut list) = (Vec::new(), false);
        for mime in mime_list(text) {
            if mime == TARGETS_MIME {
                list = true;
            } else if mimes.len() < MAX_READ_MIMES && mime.len() <= MAX_MIME_LEN {
                mimes.push(mime.to_owned());
            }
        }
        let pw = meta.effective_pw().to_owned();
        // A listing never spends a grant: a paste event's one-time password
        // must survive it for the read of the data that follows.
        let granted =
            !read_prompt_exempt(mimes.len()) && self.grants.use_grant(&pw, Direction::Read);
        Some(Request::Read(Box::new(Read {
            loc: meta.loc,
            mimes,
            list,
            id: meta.id.clone(),
            pw,
            name: meta.name.clone(),
            granted,
            terminator,
        })))
    }

    fn commit(&mut self, terminator: Terminator, replies: &mut Vec<u8>) -> Option<Request> {
        let state = self.write.take()?;
        let id = state.id.clone();
        let Ok(committed) = state.commit() else {
            // The last type's data wasn't padded right.
            Response::status(Operation::Write, Status::Einval, &id, terminator).encode(replies);
            return None;
        };
        let pw = if committed.name.is_empty() {
            String::new()
        } else {
            committed.pw
        };
        let granted = self.grants.use_grant(&pw, Direction::Write);
        Some(Request::Write(Box::new(Write {
            loc: committed.loc,
            id: committed.id,
            pw,
            name: committed.name,
            contents: committed.contents,
            granted,
            terminator,
        })))
    }

    /// End the write in flight with `status`, under the id it began with.
    fn fail(&mut self, status: Status, terminator: Terminator, replies: &mut Vec<u8>) {
        if let Some(state) = self.write.take() {
            Response::status(Operation::Write, status, &state.id, terminator).encode(replies);
        }
    }

    /// A paste event with a fresh one-time password, which may read once.
    pub(crate) fn paste_event(
        &mut self,
        primary: bool,
        available: &[String],
        random: &[u8],
    ) -> Option<Vec<u8>> {
        let pw = one_time_password(random)?;
        self.grants.grant(&pw, Direction::Read, true);
        let mut out = Vec::new();
        PasteEvent {
            primary,
            pw: &pw,
            available,
            terminator: Terminator::St,
        }
        .encode(&mut out);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(raw: &str) -> Option<Metadata> {
        Metadata::parse(raw.as_bytes()).unwrap()
    }
    fn encoded(f: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut out = Vec::new();
        f(&mut out);
        String::from_utf8(out).unwrap()
    }
    fn data_meta(mime: &str) -> Metadata {
        Metadata {
            op: Some(Operation::Wdata),
            mime: mime.into(),
            ..Metadata::default()
        }
    }
    fn begin() -> WriteState {
        WriteState::new(
            &Metadata {
                op: Some(Operation::Write),
                ..Metadata::default()
            },
            MAX_WRITE_SIZE,
        )
    }

    #[test]
    fn metadata_drops_malformed_records_and_unknown_types() {
        assert_eq!(Metadata::parse(b"").unwrap(), None);
        for raw in [
            "type=read:bare",
            "bare:type=read",
            "loc=primary",
            "type=bobr",
            "type=",
            "type=read:type=bobr",
        ] {
            assert_eq!(meta(raw), None, "{raw}");
        }
        assert_eq!(
            meta("type=bobr:type=read").unwrap().op,
            Some(Operation::Read)
        );
        assert_eq!(
            meta("type=read:bobr=kurwa").unwrap().op,
            Some(Operation::Read)
        );
        let read = meta("type=read").unwrap();
        assert_eq!((read.loc, read.id.as_str()), (Location::Standard, ""));
        assert_eq!(
            meta("type=read:loc=primary").unwrap().loc,
            Location::Primary
        );
        assert_eq!(meta("type=read:loc=bobr").unwrap().loc, Location::Standard);
    }

    #[test]
    fn metadata_sanitizes_ids_and_decodes_strict_utf8_values() {
        assert_eq!(meta("type=read:id=*a1-_+.b!").unwrap().id, "a1-_+.b");
        assert_eq!(
            meta(&format!("type=read:id={}", "x".repeat(600)))
                .unwrap()
                .id
                .len(),
            MAX_ID_LEN
        );
        // mime=text/plain, pw=Ghostty, name=BOBR KURWA
        let full =
            meta("type=wdata:mime=dGV4dC9wbGFpbg==:pw=R2hvc3R0eQ==:name=Qk9CUiBLVVJXQQ==").unwrap();
        assert_eq!(
            (full.mime.as_str(), full.pw.as_str(), full.name.as_str()),
            ("text/plain", "Ghostty", "BOBR KURWA")
        );
        assert_eq!(full.effective_pw(), "Ghostty");
        assert_eq!(
            meta("type=read:pw=R2hvc3R0eQ==").unwrap().effective_pw(),
            "",
            "no name, no password"
        );
        // Unpadded, whitespace, invalid UTF-8 (0xff), and an over-long name.
        for raw in [
            "type=wdata:mime=dGV4dC9wbGFpbg",
            "type=wdata:mime=dGV4 dC9wbGFpbg==",
            "type=wdata:mime=/w==",
        ] {
            assert_eq!(Metadata::parse(raw.as_bytes()), Err(InvalidValue), "{raw}");
        }
        let mut long = Vec::new();
        base64::encode(&[b'n'; MAX_NAME_LEN + 1], &mut long);
        let raw = format!("type=read:name={}", String::from_utf8(long).unwrap());
        assert_eq!(Metadata::parse(raw.as_bytes()), Err(InvalidValue));
        assert_eq!(
            Metadata::operation(b"type=wdata:mime=@@@"),
            Some(Operation::Wdata)
        );
        let mut pw = Vec::new();
        base64::encode(&[b'p'; MAX_PW_LEN + 1], &mut pw);
        let raw = format!("type=read:pw={}", String::from_utf8(pw).unwrap());
        assert_eq!(
            Metadata::parse(raw.as_bytes()).unwrap().unwrap().pw,
            "",
            "an over-long password is none"
        );
    }

    #[test]
    fn payloads_are_strict_padded_base64_split_on_whitespace() {
        let list = decode_payload(b"dGV4dC9wbGFpbiAgaW1hZ2UvcG5nCg==").unwrap();
        let list = String::from_utf8(list).unwrap();
        assert_eq!(
            mime_list(&list).collect::<Vec<_>>(),
            ["text/plain", "image/png"]
        );
        for bad in [&b"dGV4dA"[..], b"dGV4 dA==", b"dG=4dA==", b"dGV4dA==dA=="] {
            assert!(decode_payload(bad).is_none(), "{bad:?}");
        }
        assert_eq!(decode_payload(b"").unwrap(), b"");
        assert!(read_prompt_exempt(0) && !read_prompt_exempt(1));
        let mut out = Vec::new();
        base64::encode(b"Ghostty", &mut out);
        assert_eq!(out, b"R2hvc3R0eQ==");
    }

    #[test]
    fn writes_accumulate_chunks_types_and_reused_types() {
        let mut state = begin();
        state.data(&data_meta("text/plain"), b"R2hv").unwrap();
        state.data(&data_meta("text/plain"), b"c3R0eQ==").unwrap();
        state.data(&data_meta("text/html"), b"PGI+").unwrap(); // <b>
        let committed = state.commit().unwrap();
        let contents: Vec<(&str, &[u8])> = committed
            .contents
            .iter()
            .map(|(m, d)| (m.as_str(), &d[..]))
            .collect();
        assert_eq!(
            contents,
            [("text/plain", &b"Ghostty"[..]), ("text/html", b"<b>")]
        );

        let mut state = begin();
        state.data(&data_meta("text/plain"), b"b2xk").unwrap(); // old
        state.data(&data_meta("text/html"), b"PGI+").unwrap();
        state.data(&data_meta("text/plain"), b"bmV3").unwrap(); // new
        let committed = state.commit().unwrap();
        assert_eq!(
            &*committed.contents[0].1, b"new",
            "a reused type starts over"
        );
    }

    #[test]
    fn write_streams_split_anywhere_restart_after_padding_and_reject_corruption() {
        // "Ghostty" split mid-group.
        let mut state = begin();
        for chunk in [&b"R2"[..], b"hvc", b"3R0e", b"Q=="] {
            state.data(&data_meta("text/plain"), chunk).unwrap();
        }
        assert_eq!(&*state.commit().unwrap().contents[0].1, b"Ghostty");
        let mut state = begin();
        state.data(&data_meta("text/plain"), b"Z29vZA==").unwrap();
        state.data(&data_meta("text/plain"), b"bW9yZQ==").unwrap();
        assert_eq!(&*state.commit().unwrap().contents[0].1, b"goodmore");
        let mut state = begin();
        assert_eq!(
            state.data(&data_meta("text/plain"), b"Z29vZA==bW9y"),
            Err(WriteError::Invalid)
        );
        let mut state = begin();
        assert_eq!(
            state.data(&data_meta("text/plain"), b"R2h*"),
            Err(WriteError::Invalid)
        );
        let mut state = begin();
        state.data(&data_meta("text/plain"), b"R2h").unwrap();
        assert_eq!(
            state.commit().map(|_| ()),
            Err(WriteError::Invalid),
            "unpadded at commit"
        );
        let mut state = begin();
        state.data(&data_meta("text/plain"), b"R2h").unwrap();
        assert_eq!(
            state.data(&data_meta("text/html"), b"PGI+"),
            Err(WriteError::Invalid),
            "unpadded at a switch"
        );
    }

    #[test]
    fn write_limits_cover_every_type_together() {
        let meta_write = Metadata {
            op: Some(Operation::Write),
            ..Metadata::default()
        };
        let mut state = WriteState::new(&meta_write, 6);
        state.data(&data_meta("text/plain"), b"YWJj").unwrap(); // abc
        state.data(&data_meta("image/png"), b"ZGVm").unwrap(); // def: exactly 6
        assert_eq!(
            state.data(&data_meta("text/html"), b"Zw=="),
            Err(WriteError::TooLarge)
        );
        let mut state = WriteState::new(&meta_write, 2);
        assert_eq!(
            state.data(&data_meta("text/plain"), b"YWJj"),
            Err(WriteError::TooLarge)
        );
        let mut state = WriteState::new(&meta_write, 4);
        state.data(&data_meta("text/plain"), b"YWJj").unwrap();
        assert_eq!(
            state.data(&data_meta("text/plain"), b"ZGVm"),
            Err(WriteError::TooLarge),
            "across chunks"
        );
    }

    #[test]
    fn aliases_resolve_at_commit_chain_and_drop_without_a_target() {
        let mut state = begin();
        state.data(&data_meta("text/plain"), b"aGk=").unwrap(); // hi
        let alias = |mime: &str| Metadata {
            op: Some(Operation::Walias),
            mime: mime.into(),
            ..Metadata::default()
        };
        let mut list = Vec::new();
        base64::encode(b"UTF8_STRING  text/x-a", &mut list);
        state.alias(&alias("text/plain"), &list).unwrap();
        let mut chained = Vec::new();
        base64::encode(b"text/x-b", &mut chained);
        state.alias(&alias("text/x-a"), &chained).unwrap();
        let mut missing = Vec::new();
        base64::encode(b"image/x-none", &mut missing);
        state.alias(&alias("image/png"), &missing).unwrap();
        // Repeating an alias only moves its target.
        for _ in 0..100 {
            state.alias(&alias("text/plain"), &list).unwrap();
        }
        assert_eq!(state.aliases.len(), 4);
        let committed = state.commit().unwrap();
        let types: Vec<&str> = committed.contents.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(types, ["text/plain", "UTF8_STRING", "text/x-a", "text/x-b"]);
        assert!(committed.contents.iter().all(|(_, d)| &**d == b"hi"));
        let mut state = begin();
        let mut bad = Vec::new();
        base64::encode(b"\xff", &mut bad);
        assert_eq!(
            state.alias(&alias("text/plain"), &bad),
            Err(WriteError::Invalid)
        );
    }

    #[test]
    fn grants_are_per_direction_one_time_ones_go_with_any_check_and_the_oldest_leave() {
        let mut grants = Grants::default();
        grants.grant("pw", Direction::Read, false);
        assert!(grants.use_grant("pw", Direction::Read) && grants.use_grant("pw", Direction::Read));
        assert!(!grants.use_grant("pw", Direction::Write));
        grants.grant("pw", Direction::Write, false);
        assert!(grants.use_grant("pw", Direction::Write));
        grants.grant("otp", Direction::Read, true);
        assert!(
            !grants.use_grant("otp", Direction::Write),
            "wrong direction still spends it"
        );
        assert!(!grants.use_grant("otp", Direction::Read));
        assert!(!grants.use_grant("", Direction::Read));
        for i in 0..33 {
            grants.grant(&format!("p{i}"), Direction::Read, false);
        }
        assert!(!grants.use_grant("pw", Direction::Read), "evicted");
        assert!(grants.use_grant("p32", Direction::Read));
    }

    #[test]
    fn one_time_passwords_use_the_alphabet_and_need_enough_entropy() {
        let password = one_time_password(&(0..=255).collect::<Vec<u8>>()).unwrap();
        assert_eq!(password.len(), OTP_LEN);
        assert!(password.bytes().all(|b| OTP_ALPHABET.contains(&b)));
        assert_eq!(
            one_time_password(&[255; 64]),
            None,
            "every byte past the sampling limit"
        );
    }

    #[test]
    fn responses_follow_kittys_field_order_and_omit_empty_payloads() {
        let status =
            |op, status, id, t| encoded(|out| Response::status(op, status, id, t).encode(out));
        assert_eq!(
            status(Operation::Write, Status::Done, "", Terminator::St),
            "\x1b]5522;type=write:status=DONE\x1b\\"
        );
        assert_eq!(
            status(Operation::Write, Status::Eperm, "42", Terminator::Bel),
            "\x1b]5522;type=write:status=EPERM:id=42\x07"
        );
        let full = Response {
            op: Operation::Read,
            status: Status::Data,
            primary: true,
            id: "x",
            mime: Some("text/plain"),
            pw: Some("otp"),
            payload: b"Ghostty",
            terminator: Terminator::St,
        };
        assert_eq!(
            encoded(|out| full.encode(out)),
            "\x1b]5522;type=read:status=DATA:loc=primary:id=x:mime=dGV4dC9wbGFpbg==:pw=b3Rw;R2hvc3R0eQ==\x1b\\"
        );
    }

    #[test]
    fn reads_send_ok_listing_chunked_data_and_done() {
        let empty = ReadSuccess {
            id: "7",
            ..ReadSuccess::default()
        };
        assert_eq!(
            encoded(|out| empty.encode(out)),
            "\x1b]5522;type=read:status=OK:id=7\x1b\\\x1b]5522;type=read:status=DONE:id=7\x1b\\"
        );
        let available = ["text/plain".to_string(), "image/png".to_string()];
        let listing = ReadSuccess {
            list: true,
            available: &available,
            ..ReadSuccess::default()
        };
        assert_eq!(
            encoded(|out| listing.encode(out)),
            "\x1b]5522;type=read:status=OK\x1b\\\x1b]5522;type=read:status=DATA:mime=Lg==;dGV4dC9wbGFpbiBpbWFnZS9wbmcK\x1b\\\x1b]5522;type=read:status=DONE\x1b\\"
        );
        let none = ReadSuccess {
            list: true,
            ..ReadSuccess::default()
        };
        assert!(
            encoded(|out| none.encode(out))
                .contains("\x1b]5522;type=read:status=DATA:mime=Lg==\x1b\\")
        );
        let contents = [
            ("text/plain".to_string(), Arc::<[u8]>::from(&b"hello"[..])),
            (
                "image/png".to_string(),
                Arc::<[u8]>::from(&b"\x89PNG\r\n\x1a\n"[..]),
            ),
        ];
        let read = ReadSuccess {
            contents: &contents,
            ..ReadSuccess::default()
        };
        assert_eq!(
            encoded(|out| read.encode(out)),
            "\x1b]5522;type=read:status=OK\x1b\\\x1b]5522;type=read:status=DATA:mime=dGV4dC9wbGFpbg==;aGVsbG8=\x1b\\\x1b]5522;type=read:status=DATA:mime=aW1hZ2UvcG5n;iVBORw0KGgo=\x1b\\\x1b]5522;type=read:status=DONE\x1b\\"
        );
        let big = [(
            "text/plain".to_string(),
            Arc::<[u8]>::from(vec![b'z'; READ_CHUNK_SIZE + 1]),
        )];
        let chunked = encoded(|out| {
            ReadSuccess {
                contents: &big,
                ..ReadSuccess::default()
            }
            .encode(out)
        });
        assert_eq!(chunked.matches("\x1b]5522;").count(), 4);
    }

    #[test]
    fn paste_events_list_types_with_the_one_time_password_in_every_packet() {
        let available: Vec<String> = (0..20).map(|i| format!("type/{i}")).collect();
        let event = PasteEvent {
            primary: true,
            pw: "otp",
            available: &available,
            terminator: Terminator::St,
        };
        let text = encoded(|out| event.encode(out));
        assert!(text.starts_with("\x1b]5522;type=read:status=OK:loc=primary:pw=b3Rw\x1b\\"));
        assert_eq!(text.matches(":pw=b3Rw").count(), 3);
        let listing = text
            .split(";")
            .find(|part| part.starts_with("dHlwZS8w"))
            .unwrap();
        let listing = String::from_utf8(
            decode_payload(&listing.as_bytes()[..listing.find('\x1b').unwrap()]).unwrap(),
        )
        .unwrap();
        assert_eq!(listing.split_whitespace().count(), MAX_LISTING_MIMES);
    }
}
