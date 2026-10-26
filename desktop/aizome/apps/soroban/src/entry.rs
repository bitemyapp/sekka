//! What Soroban computes, apart from how it looks: the expression being
//! entered, the answer, the memory, the tape, and what every key does.
//!
//! An expression is a list of [`Piece`]s, each with the text the
//! evaluator reads (`src`) and the text the display shows (`shown`), so
//! `√(` can be `sqrt(` underneath, a recalled answer can carry every digit
//! while showing twelve, and ⌫ takes back a whole `sin(` at once. Real
//! expressions go to `aizome_search::calc` (the palette's evaluator);
//! Programmer mode's go to `calc::int` at the word size.

use aizome_search::calc::int::{self, Word};
use aizome_search::calc::{self, Angle};
use serde::{Deserialize, Serialize};

/// The keypad showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Basic,
    Scientific,
    Programmer,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Basic, Mode::Scientific, Mode::Programmer];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Basic => "Basic",
            Mode::Scientific => "Scientific",
            Mode::Programmer => "Programmer",
        }
    }

    pub fn index(self) -> usize {
        Mode::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }
}

/// What a piece of an expression is, for editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A digit or the decimal point.
    Digit,
    /// A binary operator.
    Op,
    /// A function and its "(" (or NOT): ⌫ takes it back whole.
    Prefix,
    Open,
    Close,
    /// ², !, %, ⁻¹.
    Postfix,
    /// A whole number put in at once: an answer, the memory, a constant.
    Value,
    /// A unary minus (±).
    Neg,
    /// The exponent's E (EE).
    Exponent,
    /// Letters, spaces and commas typed.
    Text,
}

/// A number held by a [`Kind::Value`] piece, the answer or the tape.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Num {
    Real(f64),
    /// A bit pattern, and the word and radix it was shown in.
    Int {
        bits: u64,
        word: u8,
        signed: bool,
        radix: u8,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub src: String,
    pub shown: String,
    pub kind: Kind,
    pub num: Option<Num>,
}

impl Piece {
    fn new(kind: Kind, src: impl Into<String>, shown: impl Into<String>) -> Piece {
        Piece { src: src.into(), shown: shown.into(), kind, num: None }
    }

    fn digit(d: u8) -> Piece {
        let c = char::from_digit(u32::from(d), 16).unwrap_or('0').to_ascii_uppercase();
        Piece::new(Kind::Digit, c.to_string(), c.to_string())
    }
}

/// A key of a keypad, or what a typed character means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// 0–15.
    Digit(u8),
    Point,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    Open,
    Close,
    Equals,
    Clear,
    Back,
    Negate,
    Percent,
    // Scientific.
    Second,
    Angle,
    MemClear,
    MemAdd,
    MemSub,
    MemRecall,
    Square,
    Cube,
    Exp,
    Pow10,
    Recip,
    Sqrt,
    Cbrt,
    Ln,
    Log,
    Fact,
    Sin,
    Cos,
    Tan,
    Sinh,
    Cosh,
    Tanh,
    Pi,
    Euler,
    EE,
    Rand,
    Abs,
    // Programmer.
    And,
    Or,
    Xor,
    Not,
    Shl,
    Shr,
    Rol,
    Ror,
}

/// How a key looks: its family on the keypad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Digit,
    /// A function or an editing key.
    Function,
    /// An arithmetic operator: the dye ramp's step (0, pale, to 3, deep).
    Operator(u8),
    /// A bitwise operator.
    Bitwise,
    Equals,
    /// AC.
    Clear,
}

impl Key {
    pub fn family(self) -> Family {
        match self {
            Key::Digit(_) | Key::Point => Family::Digit,
            Key::Div => Family::Operator(0),
            Key::Mul => Family::Operator(1),
            Key::Sub => Family::Operator(2),
            Key::Add => Family::Operator(3),
            Key::Equals => Family::Equals,
            Key::Clear => Family::Clear,
            Key::And | Key::Or | Key::Xor | Key::Not | Key::Shl | Key::Shr | Key::Rol | Key::Ror | Key::Mod => {
                Family::Bitwise
            }
            _ => Family::Function,
        }
    }

    /// What the key says, as (label, superscript), with 2nd held or not.
    pub fn label(self, second: bool) -> (String, Option<&'static str>) {
        let plain = |s: &str| (s.to_owned(), None);
        let sup = |s: &str, p: &'static str| (s.to_owned(), Some(p));
        match self {
            Key::Digit(d) => plain(&char::from_digit(u32::from(d), 16).unwrap_or('0').to_ascii_uppercase().to_string()),
            Key::Point => plain("."),
            Key::Add => plain("+"),
            Key::Sub => plain("−"),
            Key::Mul => plain("×"),
            Key::Div => plain("÷"),
            Key::Pow => sup("x", "y"),
            Key::Mod => plain("mod"),
            Key::Open => plain("("),
            Key::Close => plain(")"),
            Key::Equals => plain("="),
            Key::Clear => plain("AC"),
            Key::Back => plain("⌫"),
            Key::Negate => plain("±"),
            Key::Percent => plain("%"),
            Key::Second => sup("2", "nd"),
            Key::Angle => plain("Deg"),
            Key::MemClear => plain("mc"),
            Key::MemAdd => plain("m+"),
            Key::MemSub => plain("m−"),
            Key::MemRecall => plain("mr"),
            Key::Square => sup("x", "2"),
            Key::Cube => sup("x", "3"),
            Key::Exp if second => sup("2", "x"),
            Key::Exp => sup("e", "x"),
            Key::Pow10 => sup("10", "x"),
            Key::Recip => plain("1/x"),
            Key::Sqrt => plain("√x"),
            Key::Cbrt => plain("∛x"),
            Key::Ln => plain("ln"),
            Key::Log if second => plain("log₂"),
            Key::Log => plain("log₁₀"),
            Key::Fact => plain("x!"),
            Key::Sin if second => sup("sin", "−1"),
            Key::Cos if second => sup("cos", "−1"),
            Key::Tan if second => sup("tan", "−1"),
            Key::Sinh if second => sup("sinh", "−1"),
            Key::Cosh if second => sup("cosh", "−1"),
            Key::Tanh if second => sup("tanh", "−1"),
            Key::Sin => plain("sin"),
            Key::Cos => plain("cos"),
            Key::Tan => plain("tan"),
            Key::Sinh => plain("sinh"),
            Key::Cosh => plain("cosh"),
            Key::Tanh => plain("tanh"),
            Key::Pi => plain("π"),
            Key::Euler => plain("e"),
            Key::EE => plain("EE"),
            Key::Rand => plain("Rand"),
            Key::Abs => plain("|x|"),
            Key::And => plain("AND"),
            Key::Or => plain("OR"),
            Key::Xor => plain("XOR"),
            Key::Not => plain("NOT"),
            Key::Shl => plain("<<"),
            Key::Shr => plain(">>"),
            Key::Rol => plain("ROL"),
            Key::Ror => plain("ROR"),
        }
    }

    /// What assistive technology and tooltips call it.
    pub fn spoken(self, second: bool) -> String {
        let s = match self {
            Key::Digit(d) => {
                return format!("{}", char::from_digit(u32::from(d), 16).unwrap_or('0').to_ascii_uppercase());
            }
            Key::Point => "decimal point",
            Key::Add => "plus",
            Key::Sub => "minus",
            Key::Mul => "times",
            Key::Div => "divided by",
            Key::Pow => "to the power of",
            Key::Mod => "modulo",
            Key::Open => "open parenthesis",
            Key::Close => "close parenthesis",
            Key::Equals => "equals",
            Key::Clear => "all clear",
            Key::Back => "delete",
            Key::Negate => "change sign",
            Key::Percent => "percent",
            Key::Second => "second functions",
            Key::Angle => "degrees or radians",
            Key::MemClear => "memory clear",
            Key::MemAdd => "memory add",
            Key::MemSub => "memory subtract",
            Key::MemRecall => "memory recall",
            Key::Square => "square",
            Key::Cube => "cube",
            Key::Exp if second => "two to the power of",
            Key::Exp => "e to the power of",
            Key::Pow10 => "ten to the power of",
            Key::Recip => "reciprocal",
            Key::Sqrt => "square root",
            Key::Cbrt => "cube root",
            Key::Ln => "natural logarithm",
            Key::Log if second => "logarithm base two",
            Key::Log => "logarithm base ten",
            Key::Fact => "factorial",
            Key::Sin if second => "arcsine",
            Key::Cos if second => "arccosine",
            Key::Tan if second => "arctangent",
            Key::Sinh if second => "inverse hyperbolic sine",
            Key::Cosh if second => "inverse hyperbolic cosine",
            Key::Tanh if second => "inverse hyperbolic tangent",
            Key::Sin => "sine",
            Key::Cos => "cosine",
            Key::Tan => "tangent",
            Key::Sinh => "hyperbolic sine",
            Key::Cosh => "hyperbolic cosine",
            Key::Tanh => "hyperbolic tangent",
            Key::Pi => "pi",
            Key::Euler => "e",
            Key::EE => "times ten to the power of",
            Key::Rand => "random number",
            Key::Abs => "absolute value",
            Key::And => "and",
            Key::Or => "or",
            Key::Xor => "exclusive or",
            Key::Not => "not",
            Key::Shl => "shift left",
            Key::Shr => "shift right",
            Key::Rol => "rotate left",
            Key::Ror => "rotate right",
        };
        s.to_owned()
    }
}

/// One line of the paper tape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TapeEntry {
    pub expr: String,
    pub result: String,
    pub value: Num,
}

/// The most the tape keeps.
pub const TAPE_MAX: usize = 1000;

/// What the display shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    /// The big line: the expression being typed, or the answer.
    pub main: String,
    /// The small line above: the answer's expression, a preview, or an
    /// error.
    pub sub: String,
    pub error: bool,
    /// Parentheses left open, shown faintly after the expression.
    pub open: usize,
    /// Whether `main` is an answer (not an expression being typed).
    pub answer: bool,
}

/// What can be undone: the entry and the answer.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    pieces: Vec<Piece>,
    fresh: bool,
    ans: f64,
    int_ans: u64,
    from: String,
    error: Option<String>,
}

/// The calculator.
#[derive(Clone, Debug)]
pub struct Calc {
    pub mode: Mode,
    pub pieces: Vec<Piece>,
    /// The display shows an answer: a digit starts over, an operator goes
    /// on from it.
    pub fresh: bool,
    pub ans: f64,
    pub int_ans: u64,
    /// The expression the answer came from, as shown.
    pub from: String,
    pub error: Option<String>,
    pub angle: Angle,
    pub second: bool,
    pub memory: Option<f64>,
    pub radix: u32,
    pub word: Word,
    pub tape: Vec<TapeEntry>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    rng: u64,
}

impl Default for Calc {
    fn default() -> Calc {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x2545_f491, |d| d.as_nanos() as u64);
        Calc {
            mode: Mode::Basic,
            pieces: Vec::new(),
            fresh: false,
            ans: 0.0,
            int_ans: 0,
            from: String::new(),
            error: None,
            angle: Angle::Degrees,
            second: false,
            memory: None,
            radix: 10,
            word: Word::new(64, true),
            tape: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            rng: seed | 1,
        }
    }
}

/// An evaluator's error, as the display says it.
fn describe(e: &calc::Error) -> String {
    match e {
        calc::Error::Domain(s) => {
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
        calc::Error::UnknownName(n) => format!("Unknown name “{n}”"),
        calc::Error::Syntax(s) if s == "the end" || s == "nothing" => "Incomplete expression".into(),
        calc::Error::Syntax(s) if s.contains(" in base ") => format!("Not a digit: {s}"),
        calc::Error::Syntax(_) => "Not an expression Soroban can read".into(),
    }
}

/// A real number as the display writes it.
pub fn format_real(v: f64) -> String {
    calc::format(v)
}

/// Text a number was shown as, made plain for the clipboard: no thin
/// spaces, an ASCII minus, `e` for the exponent.
pub fn plain(shown: &str) -> String {
    shown.replace('\u{2009}', "").replace('−', "-").replace(" × 10^", "e")
}

impl Calc {
    // Reading.

    /// The text the evaluator reads.
    pub fn source(&self) -> String {
        self.pieces.iter().map(|p| p.src.as_str()).collect()
    }

    /// The expression as shown.
    pub fn shown_expr(&self) -> String {
        let s: String = self.pieces.iter().map(|p| p.shown.as_str()).collect();
        // Spaces typed beside an operator's own come out once.
        s.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
    }

    /// Parentheses opened and not yet closed.
    pub fn open_parens(&self) -> usize {
        let mut depth: i64 = 0;
        for p in &self.pieces {
            depth += p.src.matches('(').count() as i64;
            depth -= p.src.matches(')').count() as i64;
            depth = depth.max(0);
        }
        depth as usize
    }

    fn int_options(&self) -> (u64, Word, u32) {
        (self.int_ans, self.word, self.radix)
    }

    /// The expression's value, or why it has none.
    pub fn evaluate(&self) -> Result<Num, String> {
        let src = self.source();
        if self.mode == Mode::Programmer {
            let (ans, word, radix) = self.int_options();
            let vars = [("ans", ans)];
            let opts = int::Options { radix, word, vars: &vars };
            int::eval(&src, &opts)
                .map(|bits| Num::Int { bits, word: word.bits as u8, signed: word.signed, radix: radix as u8 })
                .map_err(|e| describe(&e))
        } else {
            let mut vars = vec![("ans", self.ans)];
            if let Some(m) = self.memory {
                vars.push(("m", m));
            }
            let opts = calc::Options { angle: self.angle, vars: &vars };
            match calc::eval_with(&src, &opts) {
                Ok(v) if !v.is_finite() => Err("Too large to show".into()),
                Ok(v) => Ok(Num::Real(if v == 0.0 { 0.0 } else { v })),
                Err(e) => Err(describe(&e)),
            }
        }
    }

    /// A number as this mode shows it.
    pub fn format(&self, n: Num) -> String {
        match (self.mode, n) {
            (Mode::Programmer, Num::Int { bits, .. }) => int::format(bits, self.radix, self.word, true),
            (Mode::Programmer, Num::Real(v)) => int::format(self.real_to_int(v), self.radix, self.word, true),
            (_, Num::Real(v)) => format_real(v),
            (_, Num::Int { bits, word, signed, .. }) => {
                format_real(int_to_real(bits, Word::new(u32::from(word), signed)))
            }
        }
    }

    /// The answer, as a number of this mode.
    pub fn answer(&self) -> Num {
        if self.mode == Mode::Programmer {
            Num::Int {
                bits: self.int_ans,
                word: self.word.bits as u8,
                signed: self.word.signed,
                radix: self.radix as u8,
            }
        } else {
            Num::Real(self.ans)
        }
    }

    /// Whether the expression is only a number (nothing to preview).
    fn is_lone_number(&self) -> bool {
        let mut it = self.pieces.iter().filter(|p| p.src.trim() != "");
        let first = it.next();
        let lone = |k: Kind| matches!(k, Kind::Digit | Kind::Value);
        match first {
            None => true,
            Some(p) if p.kind == Kind::Neg => it.all(|p| lone(p.kind)),
            Some(p) => lone(p.kind) && it.all(|p| lone(p.kind)),
        }
    }

    /// What the display shows now.
    pub fn shown(&self) -> Shown {
        if self.fresh {
            let sub = if self.from.is_empty() { String::new() } else { format!("{} =", self.from) };
            return Shown { main: self.format(self.answer()), sub, error: false, open: 0, answer: true };
        }
        let main = if self.pieces.is_empty() { "0".to_owned() } else { self.shown_expr() };
        let open = self.open_parens();
        if let Some(e) = &self.error {
            return Shown { main, sub: e.clone(), error: true, open, answer: false };
        }
        let sub = match self.evaluate() {
            Ok(n) if !self.is_lone_number() => format!("= {}", self.format(n)),
            _ => String::new(),
        };
        Shown { main, sub, error: false, open, answer: false }
    }

    /// The value in the display, for the bit toggles: the answer, the
    /// expression's value, or the last number typed.
    pub fn bits(&self) -> u64 {
        if self.fresh || self.pieces.is_empty() {
            return if self.fresh { self.int_ans } else { 0 };
        }
        match self.evaluate() {
            Ok(Num::Int { bits, .. }) => bits,
            _ => {
                let start = self.trailing_number_start();
                let digits: String =
                    self.pieces[start..].iter().filter(|p| p.kind == Kind::Digit).map(|p| p.src.as_str()).collect();
                u64::from_str_radix(&digits, self.radix).map(|v| self.word.wrap(v)).unwrap_or(0)
            }
        }
    }

    /// What ❖C copies: the answer, or the expression's value, or the
    /// expression itself.
    pub fn copy_text(&self) -> String {
        if self.fresh {
            return plain(&self.format(self.answer()));
        }
        match self.evaluate() {
            Ok(n) => plain(&self.format(n)),
            Err(_) => self.shown_expr(),
        }
    }

    /// The tape as text, a line each.
    pub fn tape_text(&self) -> String {
        let mut s = String::new();
        for e in &self.tape {
            s.push_str(&format!("{} = {}\n", e.expr, e.result));
        }
        s
    }

    // Changing.

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            pieces: self.pieces.clone(),
            fresh: self.fresh,
            ans: self.ans,
            int_ans: self.int_ans,
            from: self.from.clone(),
            error: self.error.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.pieces = s.pieces;
        self.fresh = s.fresh;
        self.ans = s.ans;
        self.int_ans = s.int_ans;
        self.from = s.from;
        self.error = s.error;
    }

    /// Remember the entry before a change, for ❖Z.
    fn remember(&mut self) {
        let s = self.snapshot();
        if self.undo.last() != Some(&s) {
            self.undo.push(s);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            self.restore(s);
        }
    }

    /// Start a new expression, from nothing.
    fn start_over(&mut self) {
        self.pieces.clear();
        self.fresh = false;
        self.error = None;
    }

    /// A piece holding `n`.
    fn value_piece(&self, n: Num) -> Piece {
        let (src, shown) = match (self.mode, n) {
            (Mode::Programmer, _) => {
                let bits = match n {
                    Num::Int { bits, .. } => self.word.wrap(bits),
                    Num::Real(v) => self.real_to_int(v),
                };
                (format!("(0x{bits:X})"), int::format(bits, self.radix, self.word, true))
            }
            (_, Num::Real(v)) => (format!("({v:?})"), format_real(v)),
            (_, Num::Int { bits, word, signed, .. }) => {
                let v = int_to_real(bits, Word::new(u32::from(word), signed));
                (format!("({v:?})"), format_real(v))
            }
        };
        let mut p = Piece::new(Kind::Value, src, shown);
        p.num = Some(n);
        p
    }

    /// Go on from the answer: the expression becomes it.
    fn continue_from_answer(&mut self) {
        let piece = self.value_piece(self.answer());
        self.pieces = vec![piece];
        self.fresh = false;
        self.error = None;
    }

    /// Where the number at the end starts (its digits, or a value), or
    /// the length if it doesn't end in one.
    fn trailing_number_start(&self) -> usize {
        let mut i = self.pieces.len();
        if let Some(p) = self.pieces.last()
            && p.kind == Kind::Value
        {
            return i - 1;
        }
        while i > 0 && matches!(self.pieces[i - 1].kind, Kind::Digit | Kind::Exponent) {
            i -= 1;
        }
        // An exponent's sign belongs to the number: 1E−3.
        if i >= 2 && self.pieces[i - 1].kind == Kind::Text && self.pieces[i - 2].kind == Kind::Exponent {
            i -= 2;
            while i > 0 && self.pieces[i - 1].kind == Kind::Digit {
                i -= 1;
            }
        }
        i
    }

    fn push(&mut self, p: Piece) {
        self.pieces.push(p);
    }

    /// Press `key`: whether the tape or the memory changed (to be saved).
    pub fn press(&mut self, key: Key) -> bool {
        match key {
            Key::Second => {
                self.second = !self.second;
                return false;
            }
            Key::Angle => {
                self.angle = if self.angle == Angle::Degrees { Angle::Radians } else { Angle::Degrees };
                return true;
            }
            Key::MemClear => {
                let had = self.memory.take().is_some();
                return had;
            }
            Key::MemAdd | Key::MemSub => {
                let v = if self.fresh || self.pieces.is_empty() {
                    Ok(Num::Real(if self.fresh { self.ans } else { 0.0 }))
                } else {
                    self.evaluate()
                };
                match v {
                    Ok(n) => {
                        let v = num_to_real(n);
                        let m = self.memory.unwrap_or(0.0);
                        self.memory = Some(if key == Key::MemAdd { m + v } else { m - v });
                        return true;
                    }
                    Err(e) => {
                        self.remember();
                        self.error = Some(e);
                        return false;
                    }
                }
            }
            _ => {}
        }
        self.remember();
        let second = self.second;
        match key {
            Key::Digit(d) => {
                if self.mode == Mode::Programmer && u32::from(d) >= self.radix {
                    return false;
                }
                if self.mode != Mode::Programmer && d > 9 {
                    return false;
                }
                if self.fresh {
                    self.start_over();
                }
                // A digit after a value or a ")" multiplies: 2π3 is fine to
                // calc, so let it be.
                self.error = None;
                self.push(Piece::digit(d));
            }
            Key::Point => {
                if self.mode == Mode::Programmer {
                    return false;
                }
                if self.fresh {
                    self.start_over();
                }
                let start = self.trailing_number_start();
                if self.pieces[start..].iter().any(|p| p.src == ".") {
                    return false;
                }
                self.error = None;
                if start == self.pieces.len() {
                    self.push(Piece::new(Kind::Digit, "0", "0"));
                }
                self.push(Piece::new(Kind::Digit, ".", "."));
            }
            Key::Add | Key::Sub => {
                self.go_on();
                let plus = key == Key::Add;
                if self.pieces.last().is_some_and(|p| p.kind == Kind::Exponent) {
                    self.push(Piece::new(Kind::Text, if plus { "+" } else { "-" }, if plus { "+" } else { "−" }));
                } else if !plus && self.pieces.is_empty() {
                    self.push(Piece::new(Kind::Neg, "-", "−"));
                } else {
                    self.replace_trailing_op();
                    self.push(Piece::new(Kind::Op, if plus { "+" } else { "-" }, if plus { " + " } else { " − " }));
                }
            }
            Key::Mul | Key::Div | Key::Pow | Key::Mod => {
                self.go_on();
                if self.pieces.is_empty() {
                    self.continue_from_answer();
                }
                self.replace_trailing_op();
                let (src, shown) = match key {
                    Key::Mul => ("*", " × "),
                    Key::Div => ("/", " ÷ "),
                    Key::Pow => ("^", "^"),
                    _ => (" mod ", " mod "),
                };
                self.push(Piece::new(Kind::Op, src, shown));
            }
            Key::And | Key::Or | Key::Xor | Key::Shl | Key::Shr | Key::Rol | Key::Ror => {
                self.go_on();
                if self.pieces.is_empty() {
                    self.continue_from_answer();
                }
                self.replace_trailing_op();
                let (src, shown) = match key {
                    Key::And => (" and ", " AND "),
                    Key::Or => (" or ", " OR "),
                    Key::Xor => (" xor ", " XOR "),
                    Key::Shl => (" << ", " << "),
                    Key::Shr => (" >> ", " >> "),
                    Key::Rol => (" rol ", " ROL "),
                    _ => (" ror ", " ROR "),
                };
                self.push(Piece::new(Kind::Op, src, shown));
            }
            Key::Not => {
                if self.fresh {
                    self.continue_from_answer();
                    self.pieces.insert(0, Piece::new(Kind::Prefix, " not ", "NOT "));
                } else {
                    self.error = None;
                    self.push(Piece::new(Kind::Prefix, " not ", "NOT "));
                }
            }
            Key::Open => {
                if self.fresh {
                    self.start_over();
                }
                self.error = None;
                self.push(Piece::new(Kind::Open, "(", "("));
            }
            Key::Close => {
                if self.fresh || self.open_parens() == 0 {
                    return false;
                }
                self.error = None;
                self.push(Piece::new(Kind::Close, ")", ")"));
            }
            Key::Equals => return self.equals(),
            Key::Clear => {
                self.start_over();
            }
            Key::Back => self.back(),
            Key::Negate => self.negate(),
            Key::Percent => self.postfix("%", "%"),
            Key::Square => self.postfix("^2", "²"),
            Key::Cube => self.postfix("^3", "³"),
            Key::Recip => self.postfix("^(-1)", "⁻¹"),
            Key::Fact => self.postfix("!", "!"),
            Key::Exp if second => self.prefix("2^(", "2^("),
            Key::Exp => self.prefix("exp(", "e^("),
            Key::Pow10 => self.prefix("10^(", "10^("),
            Key::Sqrt => self.prefix("sqrt(", "√("),
            Key::Cbrt => self.prefix("cbrt(", "∛("),
            Key::Ln => self.prefix("ln(", "ln("),
            Key::Log if second => self.prefix("log2(", "log₂("),
            Key::Log => self.prefix("log10(", "log("),
            Key::Abs => self.prefix("abs(", "abs("),
            Key::Sin if second => self.prefix("asin(", "sin⁻¹("),
            Key::Cos if second => self.prefix("acos(", "cos⁻¹("),
            Key::Tan if second => self.prefix("atan(", "tan⁻¹("),
            Key::Sinh if second => self.prefix("asinh(", "sinh⁻¹("),
            Key::Cosh if second => self.prefix("acosh(", "cosh⁻¹("),
            Key::Tanh if second => self.prefix("atanh(", "tanh⁻¹("),
            Key::Sin => self.prefix("sin(", "sin("),
            Key::Cos => self.prefix("cos(", "cos("),
            Key::Tan => self.prefix("tan(", "tan("),
            Key::Sinh => self.prefix("sinh(", "sinh("),
            Key::Cosh => self.prefix("cosh(", "cosh("),
            Key::Tanh => self.prefix("tanh(", "tanh("),
            Key::Pi => self.constant("(pi)", "π"),
            Key::Euler => self.constant("(e)", "e"),
            Key::Rand => {
                let v = self.random();
                if self.fresh {
                    self.start_over();
                }
                let p = self.value_piece(Num::Real(v));
                self.push(p);
            }
            Key::EE => {
                if self.fresh {
                    self.continue_from_answer();
                }
                let ends_in_digit = self.pieces.last().is_some_and(|p| p.kind == Kind::Digit && p.src != ".");
                if !ends_in_digit
                    || self.pieces[self.trailing_number_start()..].iter().any(|p| p.kind == Kind::Exponent)
                {
                    return false;
                }
                self.push(Piece::new(Kind::Exponent, "e", "E"));
            }
            Key::MemRecall => {
                let Some(m) = self.memory else { return false };
                if self.fresh {
                    self.start_over();
                }
                let p = self.value_piece(Num::Real(m));
                self.push(p);
            }
            Key::Second | Key::Angle | Key::MemClear | Key::MemAdd | Key::MemSub => {}
        }
        false
    }

    /// An operator after an answer goes on from it.
    fn go_on(&mut self) {
        if self.fresh {
            self.continue_from_answer();
        }
        self.error = None;
    }

    /// Two operators in a row: the second replaces the first (5 + × 3 is
    /// 5 × 3), as a keypad's do.
    fn replace_trailing_op(&mut self) {
        while self.pieces.last().is_some_and(|p| p.kind == Kind::Op || (p.kind == Kind::Neg && self.pieces.len() > 1)) {
            self.pieces.pop();
        }
    }

    fn postfix(&mut self, src: &str, shown: &str) {
        if self.fresh {
            self.continue_from_answer();
        }
        if self.pieces.is_empty() {
            return;
        }
        self.error = None;
        self.push(Piece::new(Kind::Postfix, src, shown));
    }

    /// A function: after an answer it takes the answer (√ of 16 is √(16)).
    fn prefix(&mut self, src: &str, shown: &str) {
        if self.fresh {
            self.continue_from_answer();
            self.pieces.insert(0, Piece::new(Kind::Prefix, src, shown));
            self.push(Piece::new(Kind::Close, ")", ")"));
            return;
        }
        self.error = None;
        self.push(Piece::new(Kind::Prefix, src, shown));
    }

    fn constant(&mut self, src: &str, shown: &str) {
        if self.fresh {
            self.start_over();
        }
        self.error = None;
        self.push(Piece::new(Kind::Value, src, shown));
    }

    fn random(&mut self) -> f64 {
        // xorshift64*: plenty for a Rand key.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let r = x.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (r >> 11) as f64 / (1u64 << 53) as f64
    }

    fn equals(&mut self) -> bool {
        if self.fresh || self.pieces.is_empty() {
            return false;
        }
        match self.evaluate() {
            Ok(n) => {
                let mut expr = self.shown_expr();
                for _ in 0..self.open_parens() {
                    expr.push(')');
                }
                let result = self.format(n);
                self.tape.push(TapeEntry { expr: expr.clone(), result, value: n });
                if self.tape.len() > TAPE_MAX {
                    self.tape.remove(0);
                }
                match n {
                    Num::Real(v) => self.ans = v,
                    Num::Int { bits, .. } => self.int_ans = bits,
                }
                self.from = expr;
                self.pieces.clear();
                self.fresh = true;
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    /// ⌫: the last piece goes; an answer becomes digits to edit.
    fn back(&mut self) {
        if self.error.take().is_some() && !self.fresh {
            self.pieces.pop();
            return;
        }
        if self.fresh {
            let text = match self.mode {
                Mode::Programmer => int::format(self.int_ans, self.radix, self.word, false),
                _ => plain(&format_real(self.ans)),
            };
            self.start_over();
            for c in text.chars() {
                match c {
                    '-' | '−' => self.push(Piece::new(Kind::Neg, "-", "−")),
                    'e' => self.push(Piece::new(Kind::Exponent, "e", "E")),
                    '+' => self.push(Piece::new(Kind::Text, "+", "+")),
                    c => self.push(Piece::new(Kind::Digit, c.to_string(), c.to_string())),
                }
            }
        }
        self.pieces.pop();
        // Spaces typed between words go with what they followed.
        while self.pieces.last().is_some_and(|p| p.kind == Kind::Text && p.src == " ") {
            self.pieces.pop();
        }
    }

    /// ±: the answer changes sign; or the number at the end does; or the
    /// next one will.
    fn negate(&mut self) {
        if self.fresh {
            match self.mode {
                Mode::Programmer => self.int_ans = self.word.wrap(self.int_ans.wrapping_neg()),
                _ => self.ans = -self.ans,
            }
            if !self.from.is_empty() {
                self.from = format!("−({})", self.from);
            }
            return;
        }
        self.error = None;
        let mut start = self.trailing_number_start();
        if start == self.pieces.len() && self.pieces.last().is_some_and(|p| p.kind == Kind::Close) {
            // A closed group: (…) or a function's.
            let mut depth = 0i64;
            let mut i = self.pieces.len();
            while i > 0 {
                i -= 1;
                let p = &self.pieces[i];
                depth += p.src.matches(')').count() as i64;
                depth -= p.src.matches('(').count() as i64;
                if depth <= 0 {
                    break;
                }
            }
            start = i;
        }
        if start > 0 && self.pieces[start - 1].kind == Kind::Neg {
            self.pieces.remove(start - 1);
        } else if start == self.pieces.len() && self.pieces.last().is_some_and(|p| p.kind == Kind::Neg) {
            self.pieces.pop();
        } else {
            self.pieces.insert(start, Piece::new(Kind::Neg, "-", "−"));
        }
    }

    /// A character typed: what it means in this mode.
    pub fn type_char(&mut self, c: char) -> bool {
        let prog = self.mode == Mode::Programmer;
        let key = match c {
            '0'..='9' => Some(Key::Digit(c as u8 - b'0')),
            'a'..='f' | 'A'..='F' if prog && self.radix == 16 && !self.in_word() => {
                Some(Key::Digit(c.to_ascii_lowercase() as u8 - b'a' + 10))
            }
            '.' if !prog => Some(Key::Point),
            '+' => Some(Key::Add),
            '-' | '−' => Some(Key::Sub),
            '*' | '×' | '·' => Some(Key::Mul),
            'x' | 'X' if self.after_operand() && !self.in_word() => Some(Key::Mul),
            '/' | '÷' => Some(Key::Div),
            '^' if prog => Some(Key::Xor),
            '^' => Some(Key::Pow),
            '%' if prog => Some(Key::Mod),
            '%' => Some(Key::Percent),
            '!' if prog => Some(Key::Not),
            '!' => Some(Key::Fact),
            '~' | '¬' if prog => Some(Key::Not),
            '&' if prog => Some(Key::And),
            '|' if prog => Some(Key::Or),
            '<' | '≪' if prog => Some(Key::Shl),
            '>' | '≫' if prog => Some(Key::Shr),
            '(' => Some(Key::Open),
            ')' => Some(Key::Close),
            '=' => Some(Key::Equals),
            'π' if !prog => Some(Key::Pi),
            '√' if !prog => Some(Key::Sqrt),
            _ => None,
        };
        if let Some(k) = key {
            self.press(k);
            return false;
        }
        if c.is_alphabetic() || c == ',' || c == ' ' || c == '°' || c == '_' {
            if c == ' ' {
                // A space parts words ("sin 30", "12 xor 10"). Between numbers
                // it would multiply (calc reads "12 34" as 12 × 34), so outside
                // Programmer mode it counts only after a word.
                let last = self.pieces.last();
                let after_word = last.is_some_and(|p| p.kind == Kind::Text && p.src.chars().all(char::is_alphabetic));
                if self.fresh || last.is_none_or(|p| p.src.ends_with(' ')) || (!prog && !after_word) {
                    return false;
                }
            }
            self.remember();
            if self.fresh {
                // A word after an answer: "ans", "mod", "sqrt"…
                if c.is_alphabetic() && !matches!(c, 'm') {
                    self.start_over();
                } else {
                    self.continue_from_answer();
                }
            }
            self.error = None;
            let lower = c.to_lowercase().to_string();
            self.push(Piece::new(Kind::Text, lower.clone(), lower));
            if prog {
                self.respell_word();
            }
        }
        false
    }

    /// Whether the expression ends in a number, a value or ")".
    fn after_operand(&self) -> bool {
        self.fresh
            || self
                .pieces
                .last()
                .is_some_and(|p| matches!(p.kind, Kind::Digit | Kind::Value | Kind::Close | Kind::Postfix))
    }

    /// Whether letters are being typed (so a, b, x… continue a word).
    fn in_word(&self) -> bool {
        self.pieces.last().is_some_and(|p| p.kind == Kind::Text && p.src.chars().all(char::is_alphabetic))
    }

    /// In Programmer mode, a run of letters and digits typed is either a
    /// number (shown in capitals) or a word (shown in small letters).
    fn respell_word(&mut self) {
        let mut i = self.pieces.len();
        while i > 0
            && matches!(self.pieces[i - 1].kind, Kind::Digit | Kind::Text)
            && self.pieces[i - 1].src.chars().all(|c| c.is_ascii_alphanumeric())
        {
            i -= 1;
        }
        let run: String = self.pieces[i..].iter().map(|p| p.src.as_str()).collect();
        let number = run.chars().all(|c| c.is_digit(self.radix));
        // The keypad's words in its capitals: AND, ROL.
        let keyword = matches!(
            run.to_ascii_lowercase().as_str(),
            "and" | "or" | "xor" | "not" | "nand" | "nor" | "shl" | "shr" | "rol" | "ror"
        );
        for p in &mut self.pieces[i..] {
            if number {
                p.kind = Kind::Digit;
                p.shown = p.src.to_uppercase();
            } else {
                p.kind = Kind::Text;
                p.shown = if keyword { p.src.to_uppercase() } else { p.src.to_lowercase() };
            }
        }
    }

    /// Paste `text` into the expression (or start one with it).
    pub fn paste(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.remember();
        if self.fresh {
            self.start_over();
        }
        // Spaces matter only beside words ("12 xor 10"); between digits
        // they group thousands, as thin spaces do.
        let chars: Vec<char> = text
            .chars()
            .filter(|c| !matches!(c, '\u{2009}' | '\u{202f}' | '\u{a0}'))
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect();
        let mut cleaned = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if c == ' ' {
                let before = cleaned.chars().last().filter(|b| b.is_alphanumeric());
                let after = chars[i + 1..].iter().find(|c| **c != ' ').filter(|a| a.is_alphanumeric());
                let word = before.zip(after).is_some_and(|(b, a)| b.is_alphabetic() || a.is_alphabetic());
                if !word || cleaned.ends_with(' ') {
                    continue;
                }
            }
            cleaned.push(c);
        }
        // Commas outside a function's parentheses group thousands: 1,234.
        if !cleaned.contains('(') {
            cleaned = cleaned.replace(',', "");
        }
        let undo_len = self.undo.len();
        for c in cleaned.chars() {
            self.type_char(c);
        }
        // One undo for the whole paste.
        self.undo.truncate(undo_len);
        self.redo.clear();
    }

    /// Put a tape entry's value into the expression.
    pub fn recall(&mut self, index: usize) {
        let Some(e) = self.tape.get(index) else { return };
        let n = e.value;
        self.remember();
        if self.fresh {
            self.start_over();
        }
        self.error = None;
        let p = self.value_piece(n);
        self.push(p);
    }

    pub fn clear_tape(&mut self) {
        self.tape.clear();
    }

    // Programmer mode.

    fn real_to_int(&self, v: f64) -> u64 {
        if !v.is_finite() {
            return 0;
        }
        let t = v.trunc();
        if t < 0.0 {
            self.word.from_i64(t.max(i64::MIN as f64) as i64)
        } else {
            self.word.wrap(t.min(u64::MAX as f64) as u64)
        }
    }

    /// Switch keypads, carrying the value across.
    pub fn set_mode(&mut self, mode: Mode) {
        if mode == self.mode {
            return;
        }
        let crossing = (mode == Mode::Programmer) != (self.mode == Mode::Programmer);
        if crossing {
            // The expression's value (or the answer) goes across; an
            // expression that has none is dropped.
            let value = if self.fresh || self.pieces.is_empty() { Some(self.answer()) } else { self.evaluate().ok() };
            let had_expr = !self.fresh && !self.pieces.is_empty();
            let from = if had_expr { self.shown_expr() } else { self.from.clone() };
            self.pieces.clear();
            self.error = None;
            self.undo.clear();
            self.redo.clear();
            self.mode = mode;
            match (mode, value) {
                (Mode::Programmer, Some(n)) => self.int_ans = self.real_to_int(num_to_real(n)),
                (_, Some(n)) => self.ans = num_to_real(n),
                _ => {}
            }
            self.fresh = value.is_some() && (self.fresh || had_expr);
            self.from = if self.fresh { from } else { String::new() };
            return;
        }
        self.mode = mode;
    }

    /// Show and type numbers in `radix`; numbers already typed are
    /// rewritten in it.
    pub fn set_radix(&mut self, radix: u32) {
        if radix == self.radix || !matches!(radix, 2 | 8 | 10 | 16) {
            return;
        }
        let old = self.radix;
        let mut out: Vec<Piece> = Vec::with_capacity(self.pieces.len());
        let mut i = 0;
        while i < self.pieces.len() {
            let p = &self.pieces[i];
            if p.kind == Kind::Digit {
                let mut j = i;
                while j < self.pieces.len() && self.pieces[j].kind == Kind::Digit {
                    j += 1;
                }
                let digits: String = self.pieces[i..j].iter().map(|p| p.src.as_str()).collect();
                match u64::from_str_radix(&digits, old) {
                    Ok(v) => {
                        let text = int::format(v, radix, Word::new(64, false), false);
                        out.extend(text.chars().map(|c| Piece::new(Kind::Digit, c.to_string(), c.to_string())));
                    }
                    Err(_) => out.extend(self.pieces[i..j].iter().cloned()),
                }
                i = j;
                continue;
            }
            let mut p = p.clone();
            if let (Kind::Value, Some(Num::Int { bits, .. })) = (p.kind, p.num) {
                p.shown = int::format(bits, radix, self.word, true);
            }
            out.push(p);
            i += 1;
        }
        self.pieces = out;
        self.radix = radix;
    }

    pub fn set_word(&mut self, bits: u32, signed: bool) {
        self.word = Word::new(bits, signed);
        self.int_ans = self.word.wrap(self.int_ans);
        for p in &mut self.pieces {
            if let (Kind::Value, Some(Num::Int { bits, .. })) = (p.kind, p.num) {
                p.shown = int::format(bits, self.radix, self.word, true);
            }
        }
    }

    /// Flip bit `i` of the value showing: it becomes the answer.
    pub fn toggle_bit(&mut self, i: u32) {
        if i >= self.word.bits {
            return;
        }
        self.remember();
        let v = self.bits() ^ (1u64 << i);
        self.int_ans = self.word.wrap(v);
        self.pieces.clear();
        self.fresh = true;
        self.error = None;
        self.from.clear();
    }
}

/// A pattern as a real number, read as the word says.
pub fn int_to_real(bits: u64, word: Word) -> f64 {
    if word.signed { word.to_i64(bits) as f64 } else { word.wrap(bits) as f64 }
}

pub fn num_to_real(n: Num) -> f64 {
    match n {
        Num::Real(v) => v,
        Num::Int { bits, word, signed, .. } => int_to_real(bits, Word::new(u32::from(word), signed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(c: &mut Calc, ks: &[Key]) {
        for k in ks {
            c.press(*k);
        }
    }

    fn typed(c: &mut Calc, s: &str) {
        for ch in s.chars() {
            c.type_char(ch);
        }
    }

    #[test]
    fn keys_build_expressions_and_answer() {
        let mut c = Calc::default();
        keys(&mut c, &[Key::Digit(1), Key::Digit(2), Key::Mul, Key::Digit(3)]);
        assert_eq!(c.shown().main, "12 × 3");
        assert_eq!(c.shown().sub, "= 36");
        c.press(Key::Equals);
        let s = c.shown();
        assert_eq!((s.main.as_str(), s.sub.as_str(), s.answer), ("36", "12 × 3 =", true));
        assert_eq!(c.tape.len(), 1);
        assert_eq!(c.tape[0].result, "36");
        // An operator goes on from the answer; a digit starts over.
        keys(&mut c, &[Key::Add, Key::Digit(4)]);
        assert_eq!(c.shown().main, "36 + 4");
        c.press(Key::Equals);
        assert_eq!(c.shown().main, "40");
        c.press(Key::Digit(7));
        assert_eq!(c.shown().main, "7");
    }

    #[test]
    fn answers_keep_their_precision() {
        let mut c = Calc::default();
        typed(&mut c, "1/3=");
        assert_eq!(c.shown().main, "0.333333333333");
        typed(&mut c, "*3=");
        assert_eq!(c.shown().main, "1");
        typed(&mut c, "0.1+0.2=");
        assert_eq!(c.copy_text(), "0.3");
    }

    #[test]
    fn operators_replace_each_other_and_minus_starts_negative() {
        let mut c = Calc::default();
        keys(&mut c, &[Key::Digit(5), Key::Add, Key::Mul, Key::Digit(3)]);
        assert_eq!(c.shown().main, "5 × 3");
        keys(&mut c, &[Key::Clear, Key::Sub, Key::Digit(2), Key::Add, Key::Digit(5), Key::Equals]);
        assert_eq!(c.shown().main, "3");
        // 5 × −3
        keys(&mut c, &[Key::Clear, Key::Digit(5), Key::Mul, Key::Digit(3), Key::Negate]);
        assert_eq!(c.shown().main, "5 × −3");
        c.press(Key::Negate);
        assert_eq!(c.shown().main, "5 × 3");
        c.press(Key::Equals);
        c.press(Key::Negate);
        assert_eq!(c.shown().main, "−15");
    }

    #[test]
    fn back_takes_whole_functions_and_edits_answers() {
        let mut c = Calc::default();
        c.set_mode(Mode::Scientific);
        keys(&mut c, &[Key::Sin, Key::Digit(3), Key::Digit(0)]);
        assert_eq!(c.shown().main, "sin(30");
        assert_eq!(c.shown().open, 1);
        assert_eq!(c.shown().sub, "= 0.5");
        keys(&mut c, &[Key::Back, Key::Back]);
        assert_eq!(c.shown().main, "sin(");
        c.press(Key::Back);
        assert_eq!(c.shown().main, "0");
        typed(&mut c, "125=");
        c.press(Key::Back);
        assert_eq!(c.shown().main, "12");
        assert!(!c.fresh);
    }

    #[test]
    fn scientific_keys_and_angles() {
        let mut c = Calc::default();
        c.set_mode(Mode::Scientific);
        assert_eq!(c.angle, Angle::Degrees);
        keys(&mut c, &[Key::Cos, Key::Digit(6), Key::Digit(0), Key::Close, Key::Equals]);
        assert_eq!(c.shown().main, "0.5");
        c.press(Key::Angle);
        keys(&mut c, &[Key::Clear, Key::Sin, Key::Pi, Key::Div, Key::Digit(2), Key::Equals]);
        assert_eq!(c.shown().main, "1");
        // A function after an answer takes it.
        keys(&mut c, &[Key::Digit(1), Key::Digit(6), Key::Equals, Key::Sqrt]);
        assert_eq!(c.shown().main, "√(16)");
        c.press(Key::Equals);
        assert_eq!(c.shown().main, "4");
        keys(&mut c, &[Key::Square, Key::Equals]);
        assert_eq!(c.shown().main, "16");
        c.press(Key::Second);
        assert_eq!(Key::Sin.label(true).1, Some("−1"));
        keys(&mut c, &[Key::Clear, Key::Sin, Key::Digit(1), Key::Equals]);
        assert_eq!(c.tape.last().map(|e| e.expr.as_str()), Some("sin⁻¹(1)"));
        assert!((c.ans - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        keys(&mut c, &[Key::Digit(2), Key::EE, Key::Sub, Key::Digit(3), Key::Equals]);
        assert_eq!(c.shown().main, "0.002");
        keys(&mut c, &[Key::Digit(5), Key::Fact, Key::Equals]);
        assert_eq!(c.shown().main, "120");
    }

    #[test]
    fn memory_adds_and_recalls() {
        let mut c = Calc::default();
        typed(&mut c, "6*7=");
        c.press(Key::MemAdd);
        typed(&mut c, "2");
        c.press(Key::MemAdd);
        assert_eq!(c.memory, Some(44.0));
        c.press(Key::Clear);
        keys(&mut c, &[Key::MemRecall, Key::Div, Key::Digit(4), Key::Equals]);
        assert_eq!(c.shown().main, "11");
        c.press(Key::MemSub);
        assert_eq!(c.memory, Some(33.0));
        c.press(Key::MemClear);
        assert_eq!(c.memory, None);
    }

    #[test]
    fn errors_show_and_clear() {
        let mut c = Calc::default();
        typed(&mut c, "1/0=");
        let s = c.shown();
        assert!(s.error);
        assert_eq!(s.sub, "Division by zero");
        assert_eq!(c.tape.len(), 0);
        c.press(Key::Back);
        assert!(!c.shown().error);
        assert_eq!(c.shown().main, "1 ÷");
        typed(&mut c, "4=");
        assert_eq!(c.shown().main, "0.25");
    }

    #[test]
    fn typing_words_and_pasting() {
        let mut c = Calc::default();
        typed(&mut c, "sqrt(2)^2=");
        assert_eq!(c.shown().main, "2");
        typed(&mut c, "ans*3=");
        assert_eq!(c.shown().main, "6");
        c.paste("1\u{2009}234 + 1,000");
        assert_eq!(c.shown().sub, "= 2\u{2009}234");
        c.paste("−5");
        assert_eq!(c.source(), "1234+1000-5");
        c.undo();
        assert_eq!(c.source(), "1234+1000");
        c.redo();
        assert_eq!(c.source(), "1234+1000-5");
        typed(&mut c, "2x3");
        assert_eq!(c.shown().main, "1234 + 1000 − 52 × 3");
        // Spaces between numbers don't multiply; after a word they part it.
        typed(&mut c, "=12 34 x 2 =");
        assert_eq!(c.shown().main, "2\u{2009}468");
        typed(&mut c, "sin 30 + cos 60=");
        assert_eq!(c.shown().main, "1");
    }

    #[test]
    fn programmer_mode_reads_and_shows_words() {
        let mut c = Calc::default();
        c.set_mode(Mode::Programmer);
        c.set_radix(16);
        typed(&mut c, "ff&0f");
        assert_eq!(c.shown().main, "FF AND 0F");
        c.press(Key::Equals);
        assert_eq!(c.shown().main, "F");
        assert_eq!(c.int_ans, 15);
        c.set_radix(2);
        assert_eq!(c.shown().main, "1111");
        // Digits outside the radix do nothing.
        c.press(Key::Digit(2));
        assert_eq!(c.shown().main, "1111");
        c.set_radix(10);
        keys(&mut c, &[Key::Digit(1), Key::Shl, Key::Digit(4)]);
        c.set_radix(16);
        assert_eq!(c.shown().main, "1 << 4");
        assert_eq!(c.shown().sub, "= 10");
        c.press(Key::Equals);
        c.set_word(8, true);
        keys(&mut c, &[Key::Not]);
        c.press(Key::Equals);
        assert_eq!(c.int_ans, 0xEF);
        c.set_radix(10);
        assert_eq!(c.shown().main, "−17");
        c.toggle_bit(7);
        assert_eq!(c.shown().main, "111");
        c.set_word(8, false);
        typed(&mut c, "255+1=");
        assert_eq!(c.shown().main, "0");
        // Typed words read as operators.
        typed(&mut c, "12 xor 10=");
        assert_eq!(c.shown().main, "6");
    }

    #[test]
    fn values_cross_between_modes() {
        let mut c = Calc::default();
        typed(&mut c, "40+2=");
        c.set_mode(Mode::Programmer);
        assert_eq!(c.int_ans, 42);
        assert!(c.fresh);
        typed(&mut c, "-50=");
        c.set_mode(Mode::Basic);
        assert_eq!(c.ans, -8.0);
        typed(&mut c, "7.9");
        c.set_mode(Mode::Programmer);
        assert_eq!(c.int_ans, 7);
    }

    #[test]
    fn tape_recalls_and_copies() {
        let mut c = Calc::default();
        typed(&mut c, "2+2=10*10=");
        assert_eq!(c.tape_text(), "2 + 2 = 4\n10 × 10 = 100\n");
        c.recall(0);
        typed(&mut c, "+1");
        assert_eq!(c.shown().sub, "= 5");
        assert_eq!(c.copy_text(), "5");
        c.clear_tape();
        assert!(c.tape.is_empty());
    }
}
