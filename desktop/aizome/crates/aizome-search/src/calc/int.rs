//! Integer arithmetic at a word size, as a programmer's calculator does
//! it: Soroban's Programmer mode.
//!
//! Values are bit patterns of [`Word::bits`] bits, held in a `u64` with
//! the bits above the word clear. Arithmetic wraps at the word size, as a
//! register does; division, remainder and the right shift read the
//! pattern as signed or not as the word says.
//!
//! The grammar, loosest first (C's order, with words for the operators a
//! keypad shows):
//!
//! ```text
//! or      = xor (("|" | "or" | "nor") xor)*
//! xor     = and (("^" | "xor") and)*
//! and     = shift (("&" | "and" | "nand") shift)*
//! shift   = sum (("<<" | ">>" | "shl" | "shr" | "rol" | "ror") sum)*
//! sum     = product (("+" | "-") product)*
//! product = unary (("*" | "×" | "/" | "÷" | "%" | "mod") unary)*
//! unary   = ("-" | "−" | "~" | "not" | "+") unary | power
//! power   = primary ("**" unary)?
//! primary = number | name | "(" or ")"
//! ```
//!
//! Numbers are read in the radix given, unless prefixed `0x`, `0o` or (but
//! not in hexadecimal, where it's a number) `0b`. A run of letters and
//! digits that's all digits of the radix is a number, so `dead` is one in
//! hexadecimal and `and` never is.

use super::Error;

/// A word: how many bits, and whether the top one is a sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Word {
    pub bits: u32,
    pub signed: bool,
}

impl Default for Word {
    fn default() -> Word {
        Word { bits: 64, signed: true }
    }
}

impl Word {
    pub fn new(bits: u32, signed: bool) -> Word {
        Word { bits: bits.clamp(1, 64), signed }
    }

    /// The bits the word has, set.
    pub fn mask(self) -> u64 {
        if self.bits >= 64 { u64::MAX } else { (1u64 << self.bits) - 1 }
    }

    /// `v` cut to the word.
    pub fn wrap(self, v: u64) -> u64 {
        v & self.mask()
    }

    /// The pattern read as a signed number (sign-extended).
    pub fn to_i64(self, v: u64) -> i64 {
        let v = self.wrap(v);
        if self.bits >= 64 {
            return v as i64;
        }
        let sign = 1u64 << (self.bits - 1);
        if v & sign != 0 { (v | !self.mask()) as i64 } else { v as i64 }
    }

    /// The pattern of a signed number.
    pub fn from_i64(self, v: i64) -> u64 {
        self.wrap(v as u64)
    }

    /// Whether the pattern is negative as the word reads it.
    pub fn is_negative(self, v: u64) -> bool {
        self.signed && self.to_i64(v) < 0
    }
}

/// How [`eval`] reads an expression.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    pub radix: u32,
    pub word: Word,
    /// Named values (`ans`, `m`), matched without case.
    pub vars: &'a [(&'a str, u64)],
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options { radix: 10, word: Word::default(), vars: &[] }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(u64),
    Name(String),
    /// `+ - * / % & | ^ ~`, `<` for `<<`, `>` for `>>`, `p` for `**`.
    Op(char),
    Open,
    Close,
}

fn tokenize(src: &str, radix: u32, word: Word) -> Result<Vec<Token>, Error> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() || c == '\u{2009}' => i += 1,
            c if c.is_ascii_alphanumeric() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let run: String = chars[start..i].iter().filter(|c| **c != '_').collect();
                out.push(word_token(&run, radix, word)?);
            }
            '<' | '>' | '≪' | '≫' => {
                let op = if matches!(c, '<' | '≪') { '<' } else { '>' };
                if matches!(c, '<' | '>') {
                    if chars.get(i + 1) != Some(&c) {
                        return Err(Error::Syntax(c.to_string()));
                    }
                    i += 1;
                }
                i += 1;
                out.push(Token::Op(op));
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push(Token::Op('p'));
                i += 2;
            }
            '+' | '-' | '−' | '*' | '×' | '·' | '/' | '÷' | '%' | '&' | '|' | '^' | '~' | '¬' => {
                out.push(Token::Op(match c {
                    '−' => '-',
                    '×' | '·' => '*',
                    '÷' => '/',
                    '¬' => '~',
                    c => c,
                }));
                i += 1;
            }
            '(' => {
                out.push(Token::Open);
                i += 1;
            }
            ')' => {
                out.push(Token::Close);
                i += 1;
            }
            c => return Err(Error::Syntax(c.to_string())),
        }
    }
    Ok(out)
}

/// A run of letters and digits: a number (in the radix, or by its
/// prefix) or a name.
fn word_token(run: &str, radix: u32, word: Word) -> Result<Token, Error> {
    let lower = run.to_ascii_lowercase();
    let long = lower.len() > 2;
    let (r, digits) = if long && lower.starts_with("0x") {
        (16, &run[2..])
    } else if long && lower.starts_with("0o") {
        (8, &run[2..])
    } else if long && radix != 16 && lower.starts_with("0b") {
        (2, &run[2..])
    } else {
        (radix, run)
    };
    if digits.chars().all(|c| c.is_digit(r)) {
        let v = u128::from_str_radix(digits, r).map_err(|_| Error::Domain("number too large"))?;
        if v > u128::from(word.mask()) {
            return Err(Error::Domain("number too large for the word size"));
        }
        return Ok(Token::Num(v as u64));
    }
    if run.starts_with(|c: char| c.is_ascii_digit()) {
        // "19" in octal, "2a" in decimal.
        let bad = digits.chars().find(|c| !c.is_digit(r)).unwrap_or('?');
        return Err(Error::Syntax(format!("{bad} in base {r}")));
    }
    Ok(Token::Name(lower))
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    opts: Options<'a>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn word(&self) -> Word {
        self.opts.word
    }

    /// Whether the next token is the operator `op` or the name `name`; if
    /// so, it's taken.
    fn take(&mut self, op: Option<char>, names: &[&str]) -> Option<String> {
        let hit = match self.peek() {
            Some(Token::Op(c)) if Some(*c) == op => Some(c.to_string()),
            Some(Token::Name(n)) if names.contains(&n.as_str()) => Some(n.clone()),
            _ => None,
        };
        if hit.is_some() {
            self.at += 1;
        }
        hit
    }

    fn or(&mut self) -> Result<u64, Error> {
        let w = self.word();
        let mut v = self.xor()?;
        loop {
            if self.take(Some('|'), &["or"]).is_some() {
                v = w.wrap(v | self.xor()?);
            } else if self.take(None, &["nor"]).is_some() {
                v = w.wrap(!(v | self.xor()?));
            } else {
                return Ok(v);
            }
        }
    }

    fn xor(&mut self) -> Result<u64, Error> {
        let mut v = self.and()?;
        while self.take(Some('^'), &["xor"]).is_some() {
            v ^= self.and()?;
        }
        Ok(v)
    }

    fn and(&mut self) -> Result<u64, Error> {
        let w = self.word();
        let mut v = self.shift()?;
        loop {
            if self.take(Some('&'), &["and"]).is_some() {
                v &= self.shift()?;
            } else if self.take(None, &["nand"]).is_some() {
                v = w.wrap(!(v & self.shift()?));
            } else {
                return Ok(v);
            }
        }
    }

    fn shift(&mut self) -> Result<u64, Error> {
        let mut v = self.sum()?;
        loop {
            let op = if self.take(Some('<'), &["shl", "lsh"]).is_some() {
                '<'
            } else if self.take(Some('>'), &["shr", "rsh"]).is_some() {
                '>'
            } else if self.take(None, &["rol"]).is_some() {
                'l'
            } else if self.take(None, &["ror"]).is_some() {
                'r'
            } else {
                return Ok(v);
            };
            let n = self.sum()?;
            v = shift(self.word(), v, op, n)?;
        }
    }

    fn sum(&mut self) -> Result<u64, Error> {
        let w = self.word();
        let mut v = self.product()?;
        loop {
            if self.take(Some('+'), &[]).is_some() {
                v = w.wrap(v.wrapping_add(self.product()?));
            } else if self.take(Some('-'), &[]).is_some() {
                v = w.wrap(v.wrapping_sub(self.product()?));
            } else {
                return Ok(v);
            }
        }
    }

    fn product(&mut self) -> Result<u64, Error> {
        let w = self.word();
        let mut v = self.unary()?;
        loop {
            if self.take(Some('*'), &[]).is_some() {
                v = w.wrap(v.wrapping_mul(self.unary()?));
            } else if self.take(Some('/'), &["div"]).is_some() {
                v = divide(w, v, self.unary()?, false)?;
            } else if self.take(Some('%'), &["mod"]).is_some() {
                v = divide(w, v, self.unary()?, true)?;
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> Result<u64, Error> {
        let w = self.word();
        if self.take(Some('-'), &[]).is_some() {
            return Ok(w.wrap(self.unary()?.wrapping_neg()));
        }
        if self.take(Some('~'), &["not"]).is_some() {
            return Ok(w.wrap(!self.unary()?));
        }
        if self.take(Some('+'), &[]).is_some() {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<u64, Error> {
        let w = self.word();
        let base = self.primary()?;
        if self.take(Some('p'), &[]).is_some() {
            let exp = self.unary()?;
            if w.is_negative(exp) {
                return Err(Error::Domain("negative exponent"));
            }
            let e = u32::try_from(exp).unwrap_or(u32::MAX);
            return Ok(w.wrap(base.wrapping_pow(e)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<u64, Error> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        match t {
            Some(Token::Num(v)) => Ok(v),
            Some(Token::Open) => {
                let v = self.or()?;
                match self.peek() {
                    Some(Token::Close) => {
                        self.at += 1;
                        Ok(v)
                    }
                    // A missing ")" at the end is forgiven while typing.
                    None => Ok(v),
                    Some(t) => Err(Error::Syntax(format!("{t:?}"))),
                }
            }
            Some(Token::Name(n)) => match self.opts.vars.iter().find(|(name, _)| name.eq_ignore_ascii_case(&n)) {
                Some((_, v)) => Ok(self.word().wrap(*v)),
                None => Err(Error::UnknownName(n)),
            },
            Some(t) => Err(Error::Syntax(format!("{t:?}"))),
            None => Err(Error::Syntax("the end".into())),
        }
    }
}

fn divide(w: Word, a: u64, b: u64, remainder: bool) -> Result<u64, Error> {
    if w.wrap(b) == 0 {
        return Err(Error::Domain("division by zero"));
    }
    Ok(if w.signed {
        let (x, y) = (w.to_i64(a), w.to_i64(b));
        w.from_i64(if remainder { x.wrapping_rem(y) } else { x.wrapping_div(y) })
    } else if remainder {
        a % b
    } else {
        a / b
    })
}

/// Shift or rotate `v` by `n` within the word: `<`, `>`, `l`eft or
/// `r`ight rotation.
fn shift(w: Word, v: u64, op: char, n: u64) -> Result<u64, Error> {
    if w.is_negative(n) {
        return Err(Error::Domain("negative shift"));
    }
    let bits = u64::from(w.bits);
    Ok(match op {
        '<' if n >= bits => 0,
        '<' => w.wrap(v << n),
        '>' if w.signed => {
            let s = w.to_i64(v);
            w.from_i64(if n >= 64 { if s < 0 { -1 } else { 0 } } else { s >> n })
        }
        '>' if n >= bits => 0,
        '>' => v >> n,
        _ => {
            let k = (n % bits) as u32;
            if k == 0 {
                return Ok(v);
            }
            let b = w.bits;
            let (l, r) = if op == 'l' { (k, b - k) } else { (b - k, k) };
            w.wrap((v << l) | (v >> r))
        }
    })
}

/// Evaluates an integer expression as `opts` say: the bit pattern of its
/// value.
pub fn eval(src: &str, opts: &Options) -> Result<u64, Error> {
    let tokens = tokenize(src, opts.radix, opts.word)?;
    if tokens.is_empty() {
        return Err(Error::Syntax("nothing".into()));
    }
    let mut p = Parser { tokens, at: 0, opts: *opts };
    let v = p.or()?;
    if p.at < p.tokens.len() {
        return Err(Error::Syntax(format!("{:?}", p.tokens[p.at])));
    }
    Ok(opts.word.wrap(v))
}

/// The digits of `v` in `radix` as the word reads it: negative decimals
/// with a minus sign (U+2212) when the word is signed; other radixes as
/// the bare pattern. With `group`, digits are grouped with thin spaces:
/// thousands in decimal, fours in hexadecimal and binary, threes in octal.
pub fn format(v: u64, radix: u32, word: Word, group: bool) -> String {
    let v = word.wrap(v);
    let (negative, magnitude) =
        if radix == 10 && word.is_negative(v) { (true, word.to_i64(v).unsigned_abs()) } else { (false, v) };
    let digits = match radix {
        2 => format!("{magnitude:b}"),
        8 => format!("{magnitude:o}"),
        16 => format!("{magnitude:X}"),
        _ => format!("{magnitude}"),
    };
    let size = match radix {
        10 | 8 => 3,
        _ => 4,
    };
    let mut out = String::new();
    if negative {
        out.push('−');
    }
    let n = digits.len();
    for (i, c) in digits.chars().enumerate() {
        if group && i > 0 && (n - i) % size == 0 {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(radix: u32, bits: u32, signed: bool) -> Options<'static> {
        Options { radix, word: Word::new(bits, signed), vars: &[] }
    }

    fn ev(src: &str, radix: u32, bits: u32, signed: bool) -> u64 {
        eval(src, &opts(radix, bits, signed)).unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    #[test]
    fn reads_numbers_in_the_radix() {
        assert_eq!(ev("ff + 1", 16, 64, false), 256);
        assert_eq!(ev("dead", 16, 64, false), 0xdead);
        assert_eq!(ev("0x10 + 0o10 + 0b10 + 10", 10, 64, false), 16 + 8 + 2 + 10);
        // In hexadecimal, 0b1 is a number.
        assert_eq!(ev("0b1", 16, 64, false), 0xb1);
        assert_eq!(ev("17", 8, 64, false), 15);
        assert_eq!(ev("1010", 2, 64, false), 10);
        assert!(eval("19", &opts(8, 64, false)).is_err());
        assert!(eval("2", &opts(2, 64, false)).is_err());
        assert!(eval("256", &opts(10, 8, false)).is_err());
        assert_eq!(ev("255", 10, 8, true), 255);
    }

    #[test]
    fn bitwise_and_shifts() {
        assert_eq!(ev("0xF0 and 0x3C", 10, 8, false), 0x30);
        assert_eq!(ev("F0 | 0F", 16, 8, false), 0xFF);
        assert_eq!(ev("ff xor 0f", 16, 8, false), 0xF0);
        assert_eq!(ev("ff ^ 0f", 16, 8, false), 0xF0);
        assert_eq!(ev("not 0", 10, 8, false), 0xFF);
        assert_eq!(ev("~0", 10, 16, false), 0xFFFF);
        assert_eq!(ev("0xF0 nand 0x3C", 10, 8, false), 0xCF);
        assert_eq!(ev("0xF0 nor 0x0C", 10, 8, false), 0x03);
        assert_eq!(ev("1 << 4", 10, 8, false), 16);
        assert_eq!(ev("1 << 8", 10, 8, false), 0);
        assert_eq!(ev("0x80 >> 7", 10, 8, false), 1);
        // Arithmetic shift when signed.
        assert_eq!(ev("0x80 >> 7", 10, 8, true), 0xFF);
        assert_eq!(ev("0x81 rol 1", 10, 8, false), 0x03);
        assert_eq!(ev("0x81 ror 1", 10, 8, false), 0xC0);
        assert_eq!(ev("1 ror 1", 10, 64, false), 1 << 63);
        // C's precedence: shifts under sums, and under & | ^.
        assert_eq!(ev("1 << 2 + 1", 10, 32, false), 8);
        assert_eq!(ev("6 & 3 | 8", 10, 32, false), 10);
    }

    #[test]
    fn wraps_at_the_word() {
        assert_eq!(ev("255 + 1", 10, 8, false), 0);
        assert_eq!(ev("-1", 10, 8, false), 0xFF);
        assert_eq!(ev("-1", 10, 64, true), u64::MAX);
        assert_eq!(ev("0x7F + 1", 10, 8, true), 0x80);
        assert_eq!(ev("-7 / 2", 10, 8, true), Word::new(8, true).from_i64(-3));
        assert_eq!(ev("-7 % 2", 10, 8, true), Word::new(8, true).from_i64(-1));
        assert_eq!(ev("0xF9 / 2", 10, 8, false), 0x7C);
        assert_eq!(ev("2 ** 10", 10, 16, false), 1024);
        assert_eq!(ev("2 ** 16", 10, 16, false), 0);
        assert!(matches!(eval("1 / 0", &opts(10, 32, true)), Err(Error::Domain(_))));
        assert!(matches!(eval("1 mod 0", &opts(10, 32, true)), Err(Error::Domain(_))));
        assert_eq!(ev("(1 + 2", 10, 8, false), 3);
        let vars = Options { radix: 16, word: Word::new(16, false), vars: &[("ans", 0x10)] };
        assert_eq!(eval("ans * 2", &vars), Ok(0x20));
    }

    #[test]
    fn formats_by_radix_and_sign() {
        let w8 = Word::new(8, true);
        assert_eq!(format(0xFF, 10, w8, true), "−1");
        assert_eq!(format(0xFF, 10, Word::new(8, false), true), "255");
        assert_eq!(format(0xFF, 16, w8, true), "FF");
        assert_eq!(format(0xDEADBEEF, 16, Word::new(32, false), true), "DEAD\u{2009}BEEF");
        assert_eq!(format(0b101101, 2, w8, true), "10\u{2009}1101");
        assert_eq!(format(0o1234, 8, Word::new(16, false), true), "1\u{2009}234");
        assert_eq!(format(1234567, 10, Word::new(32, true), true), "1\u{2009}234\u{2009}567");
        assert_eq!(format(1234567, 10, Word::new(32, true), false), "1234567");
        let w64 = Word::new(64, true);
        assert_eq!(format(u64::MAX, 10, Word::new(64, false), false), "18446744073709551615");
        assert_eq!(format(1 << 63, 10, w64, false), "−9223372036854775808");
        assert_eq!(w64.to_i64(u64::MAX), -1);
        assert_eq!(Word::new(16, true).to_i64(0x8000), -32768);
    }
}
