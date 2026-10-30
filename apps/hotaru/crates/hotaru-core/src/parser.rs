//! Incremental DEC/ANSI parsing. Parameter and intermediate storage is inline;
//! only OSC strings allocate, with a strict size bound and discard-until-ST on
//! overflow. No chunk boundary is semantically significant.

const MAX_PARAMS: usize = 32;
// Ghostty's fixed capture excludes the command prefix and its semicolon.
// Clipboard OSC 52 and Kitty's OSC 5522 have a separate allocating cap for
// their much larger payloads.
const MAX_OSC_BODY: usize = 2048;
// A Linux PATH_MAX path can expand threefold when percent-encoded in OSC 7.
const MAX_CWD_BODY: usize = 16 * 1024;
// The pinned allocating capture also needs one byte for its sentinel NUL.
const MAX_CLIPBOARD_BODY: usize = 8 * 1024 * 1024 - 1;
// OSC 5522's capture has no sentinel: its whole 8 MiB is the body.
const MAX_KITTY_CLIPBOARD_BODY: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Param {
    pub value: u16,
    pub colon: bool,
    pub present: bool,
}

pub(crate) struct Csi<'a> {
    pub params: &'a [Param],
    pub has_parameters: bool,
    pub private: Option<u8>,
    pub intermediates: &'a [u8],
    pub final_byte: u8,
}

pub(crate) trait Handler {
    fn print(&mut self, ch: char);
    fn execute(&mut self, byte: u8);
    fn esc(&mut self, intermediates: &[u8], final_byte: u8);
    fn csi(&mut self, csi: Csi<'_>);
    fn osc(&mut self, bytes: &[u8], bell_terminated: bool);
    fn dcs(&mut self, header: Csi<'_>, bytes: &[u8]);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    Csi,
    DcsEntry,
    DcsString,
    DcsEscape,
    IgnoreCsi,
    Osc,
    OscEscape,
    IgnoreString,
    IgnoreStringEscape,
}

#[derive(Default)]
pub(crate) struct Parser {
    state: State,
    params: [Param; MAX_PARAMS],
    param_count: usize,
    has_parameters: bool,
    private: Option<u8>,
    intermediates: [u8; 2],
    intermediate_count: usize,
    osc: Vec<u8>,
    osc_overflow: bool,
    osc_body: bool,
    osc_limit: usize,
    dcs_final: u8,
    utf8_value: u32,
    utf8_left: u8,
    utf8_min: u32,
}

impl Parser {
    pub fn ground(&self) -> bool {
        self.state == State::Ground && self.utf8_left == 0
    }

    pub fn advance(&mut self, byte: u8, out: &mut impl Handler) {
        if self.state == State::Ground && self.utf8_left != 0 {
            // Reject invalid prefixes immediately, then reprocess the rejected
            // byte. This is Unicode's maximal-subpart replacement behavior:
            // ED A0 80 becomes three replacements, not one surrogate scalar.
            let allowed = byte & 0xc0 == 0x80
                && !(self.utf8_left == 2
                    && self.utf8_min == 0x800
                    && ((self.utf8_value == 0 && byte < 0xa0)
                        || (self.utf8_value == 0x0d && byte > 0x9f)))
                && !(self.utf8_left == 3
                    && self.utf8_min == 0x10000
                    && ((self.utf8_value == 0 && byte < 0x90)
                        || (self.utf8_value == 4 && byte > 0x8f)));
            if allowed {
                self.utf8_value = (self.utf8_value << 6) | u32::from(byte & 0x3f);
                self.utf8_left -= 1;
                if self.utf8_left == 0 {
                    let ch = if self.utf8_value >= self.utf8_min {
                        char::from_u32(self.utf8_value)
                    } else {
                        None
                    };
                    out.print(ch.unwrap_or('\u{fffd}'));
                }
                return;
            }
            self.utf8_left = 0;
            out.print('\u{fffd}');
            // Reprocess this byte; it may be ESC, ASCII, or a new UTF-8 lead.
        }
        if matches!(byte, 0x18 | 0x1a) {
            self.state = State::Ground;
            self.osc.clear();
            return;
        }
        match self.state {
            State::Ground => match byte {
                0x1b => self.escape(),
                0x00..=0x1f => out.execute(byte),
                0x20..=0x7e => out.print(byte as char),
                0x7f => {}
                0xc2..=0xdf => self.utf8_start(byte & 0x1f, 1, 0x80),
                0xe0..=0xef => self.utf8_start(byte & 0x0f, 2, 0x800),
                0xf0..=0xf4 => self.utf8_start(byte & 0x07, 3, 0x10000),
                _ => out.print('\u{fffd}'),
            },
            State::Escape => match byte {
                0x1b => self.escape(),
                0x00..=0x1f => out.execute(byte),
                b'[' | b'P' if self.intermediate_count == 0 => {
                    self.state = if byte == b'[' {
                        State::Csi
                    } else {
                        State::DcsEntry
                    };
                    self.params.fill(Param::default());
                    self.param_count = 1;
                    self.has_parameters = false;
                    self.private = None;
                }
                b']' if self.intermediate_count == 0 => {
                    self.state = State::Osc;
                    self.osc.clear();
                    self.osc_overflow = false;
                    self.osc_body = false;
                    self.osc_limit = MAX_OSC_BODY;
                }
                b'X' | b'^' | b'_' if self.intermediate_count == 0 => {
                    self.state = State::IgnoreString
                }
                0x20..=0x2f => {
                    if self.intermediate_count < 2 {
                        self.intermediates[self.intermediate_count] = byte;
                        self.intermediate_count += 1;
                    } else {
                        self.state = State::IgnoreCsi;
                    }
                }
                0x30..=0x7e => {
                    out.esc(&self.intermediates[..self.intermediate_count], byte);
                    self.state = State::Ground;
                }
                _ => {}
            },
            State::Csi | State::DcsEntry => match byte {
                0x1b => self.escape(),
                0x00..=0x1f => out.execute(byte),
                b'0'..=b'9' if self.intermediate_count == 0 => {
                    self.has_parameters = true;
                    let p = &mut self.params[self.param_count - 1];
                    p.present = true;
                    p.value = p
                        .value
                        .saturating_mul(10)
                        .saturating_add(u16::from(byte - b'0'));
                }
                b';' | b':' if self.intermediate_count == 0 => {
                    self.has_parameters = true;
                    if self.param_count == MAX_PARAMS {
                        self.ignore_control();
                    } else {
                        self.params[self.param_count - 1].colon = byte == b':';
                        self.param_count += 1;
                    }
                }
                0x3c..=0x3f
                    if self.param_count == 1
                        && self.params[0].value == 0
                        && self.private.is_none()
                        && self.intermediate_count == 0 =>
                {
                    self.private = Some(byte)
                }
                0x20..=0x2f => {
                    if self.intermediate_count == 2 {
                        self.ignore_control();
                    } else {
                        self.intermediates[self.intermediate_count] = byte;
                        self.intermediate_count += 1;
                    }
                }
                0x40..=0x7e => {
                    if self.state == State::DcsEntry {
                        self.dcs_final = byte;
                        self.osc.clear();
                        self.osc_overflow = false;
                        self.state = State::DcsString;
                    } else {
                        out.csi(Csi {
                            params: &self.params[..self.complete_param_count()],
                            has_parameters: self.has_parameters,
                            private: self.private,
                            intermediates: &self.intermediates[..self.intermediate_count],
                            final_byte: byte,
                        });
                        self.state = State::Ground;
                    }
                }
                0x7f => {}
                _ => self.ignore_control(),
            },
            State::IgnoreCsi => match byte {
                0x1b => self.escape(),
                0x00..=0x1f => out.execute(byte),
                0x40..=0x7e => self.state = State::Ground,
                _ => {}
            },
            State::Osc => match byte {
                0x07 => self.end_osc(out, true),
                0x1b => self.state = State::OscEscape,
                0x00..=0x06 | 0x08..=0x1f => {}
                _ => {
                    if !self.osc_body && !self.osc_overflow && byte == b';' {
                        let body_limit = if self.osc == b"52" {
                            MAX_CLIPBOARD_BODY
                        } else if self.osc == b"5522" {
                            MAX_KITTY_CLIPBOARD_BODY
                        } else if self.osc == b"7" {
                            MAX_CWD_BODY
                        } else {
                            MAX_OSC_BODY
                        };
                        self.osc_limit = self.osc.len() + 1 + body_limit;
                        self.osc_body = true;
                    }
                    self.capture_byte(byte, self.osc_limit);
                }
            },
            State::OscEscape => {
                if byte == b'\\' {
                    self.end_osc(out, false);
                } else {
                    self.osc.clear();
                    self.escape();
                    self.advance(byte, out);
                }
            }
            State::DcsString => match byte {
                0x1b => self.state = State::DcsEscape,
                0x00..=0x1f | 0x7f => {}
                _ => {
                    self.capture_byte(byte, 8192);
                }
            },
            State::DcsEscape => {
                if byte == b'\\' {
                    if !self.osc_overflow {
                        out.dcs(
                            Csi {
                                params: &self.params[..self.complete_param_count()],
                                has_parameters: self.has_parameters,
                                private: self.private,
                                intermediates: &self.intermediates[..self.intermediate_count],
                                final_byte: self.dcs_final,
                            },
                            &self.osc,
                        );
                    }
                    self.osc.clear();
                    self.state = State::Ground;
                } else {
                    self.osc.clear();
                    self.escape();
                    self.advance(byte, out);
                }
            }
            State::IgnoreString => {
                if byte == 0x1b {
                    self.state = State::IgnoreStringEscape;
                }
            }
            State::IgnoreStringEscape => {
                if byte == b'\\' {
                    self.state = State::Ground;
                } else {
                    self.escape();
                    self.advance(byte, out);
                }
            }
        }
    }

    fn escape(&mut self) {
        self.state = State::Escape;
        self.intermediate_count = 0;
    }
    fn complete_param_count(&self) -> usize {
        self.param_count
            - usize::from(self.param_count > 1 && !self.params[self.param_count - 1].present)
    }
    fn capture_byte(&mut self, byte: u8, limit: usize) {
        if self.osc_overflow {
            return;
        }
        if self.osc.len() >= limit {
            self.osc_overflow = true;
            self.osc.clear();
            return;
        }
        if self.osc.len() == self.osc.capacity() {
            // Vec::push's doubling would allocate 16 MiB for an 8-MiB body
            // plus its three-byte command prefix. Keep requested capacity
            // inside the precise capture bound, and discard on allocation error.
            let capacity = self.osc.capacity().saturating_mul(2).max(64).min(limit);
            if self
                .osc
                .try_reserve_exact(capacity - self.osc.len())
                .is_err()
            {
                self.osc_overflow = true;
                self.osc.clear();
                return;
            }
        }
        self.osc.push(byte);
    }
    fn ignore_control(&mut self) {
        // An invalid DCS header still owns a string payload. CSI_IGNORE ends
        // at the final header byte, which would print that payload as text.
        self.state = if self.state == State::DcsEntry {
            State::IgnoreString
        } else {
            State::IgnoreCsi
        };
    }
    fn utf8_start(&mut self, value: u8, left: u8, min: u32) {
        self.utf8_value = u32::from(value);
        self.utf8_left = left;
        self.utf8_min = min;
    }
    fn end_osc(&mut self, out: &mut impl Handler, bell_terminated: bool) {
        if !self.osc_overflow {
            out.osc(&self.osc, bell_terminated);
        }
        self.osc.clear();
        self.state = State::Ground;
    }
}
