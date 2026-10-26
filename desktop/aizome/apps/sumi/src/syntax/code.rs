//! A small tokenizer for code: one loop over a line's bytes, ruled by a
//! language's [`Spec`] (its keywords, comments and quotes), carrying block
//! comments and multi-line strings to the next line in a [`Mode`].

use super::{Kind, Language, Mode, Out};

/// The few things that set a language apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    Rust,
    Python,
    Js,
    C,
    Go,
    Nix,
    Shell,
    Toml,
    Json,
    Yaml,
}

/// A language's rules.
pub struct Spec {
    pub flavour: Flavour,
    pub keywords: &'static [&'static str],
    pub types: &'static [&'static str],
    pub constants: &'static [&'static str],
    pub line_comments: &'static [&'static str],
    pub block_comment: Option<(&'static str, &'static str)>,
    pub nested_comments: bool,
    /// Characters that open strings.
    pub quotes: &'static [u8],
    /// Those of them whose strings may run over lines.
    pub multiline: &'static [u8],
    /// Those of them whose strings have no backslash escapes.
    pub raw: &'static [u8],
    /// `"""` and `'''` (Python, TOML).
    pub triple: bool,
    /// Capitalised names are types (Rust, JavaScript, Python).
    pub capitalized_types: bool,
    /// A name followed by `(` is a call.
    pub calls: bool,
}

const RUST: Spec = Spec {
    flavour: Flavour::Rust,
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "const",
        "continue",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "fn",
        "for",
        "gen",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "macro_rules",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "Self",
        "static",
        "struct",
        "super",
        "trait",
        "try",
        "type",
        "union",
        "unsafe",
        "use",
        "where",
        "while",
        "yield",
    ],
    types: &[
        "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
        "f32", "f64",
    ],
    constants: &["true", "false"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: true,
    quotes: b"\"",
    multiline: b"\"",
    raw: b"",
    triple: false,
    capitalized_types: true,
    calls: true,
};

const PYTHON: Spec = Spec {
    flavour: Flavour::Python,
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def", "del", "elif", "else",
        "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "match", "nonlocal", "not",
        "or", "pass", "raise", "return", "try", "type", "while", "with", "yield",
    ],
    types: &["int", "float", "str", "bool", "bytes", "list", "dict", "set", "tuple", "object", "complex", "frozenset"],
    constants: &["True", "False", "None", "self", "cls", "NotImplemented", "Ellipsis"],
    line_comments: &["#"],
    block_comment: None,
    nested_comments: false,
    quotes: b"\"'",
    multiline: b"",
    raw: b"",
    triple: true,
    capitalized_types: true,
    calls: true,
};

const JS_KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

const JAVASCRIPT: Spec = Spec {
    flavour: Flavour::Js,
    keywords: JS_KEYWORDS,
    types: &[],
    constants: &["true", "false", "null", "undefined", "NaN", "Infinity"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    quotes: b"\"'`",
    multiline: b"`",
    raw: b"",
    triple: false,
    capitalized_types: true,
    calls: true,
};

const TYPESCRIPT: Spec = Spec {
    flavour: Flavour::Js,
    keywords: &[
        "abstract",
        "as",
        "asserts",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "declare",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "finally",
        "for",
        "from",
        "function",
        "get",
        "if",
        "implements",
        "import",
        "in",
        "infer",
        "instanceof",
        "interface",
        "is",
        "keyof",
        "let",
        "module",
        "namespace",
        "new",
        "of",
        "override",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "satisfies",
        "set",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "try",
        "type",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
    ],
    types: &["string", "number", "boolean", "any", "unknown", "never", "object", "symbol", "bigint"],
    constants: &["true", "false", "null", "undefined", "NaN", "Infinity"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    quotes: b"\"'`",
    multiline: b"`",
    raw: b"",
    triple: false,
    capitalized_types: true,
    calls: true,
};

const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "extern",
    "for",
    "goto",
    "if",
    "inline",
    "register",
    "restrict",
    "return",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "volatile",
    "while",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Generic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "alignas",
    "alignof",
    "static_assert",
    "thread_local",
    "typeof",
];

const C_TYPES: &[&str] = &[
    "void",
    "char",
    "short",
    "int",
    "long",
    "float",
    "double",
    "signed",
    "unsigned",
    "bool",
    "_Bool",
    "size_t",
    "ssize_t",
    "ptrdiff_t",
    "intptr_t",
    "uintptr_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "wchar_t",
    "char8_t",
    "char16_t",
    "char32_t",
    "FILE",
    "off_t",
    "pid_t",
];

const C: Spec = Spec {
    flavour: Flavour::C,
    keywords: C_KEYWORDS,
    types: C_TYPES,
    constants: &["NULL", "true", "false", "nullptr", "EOF", "stdin", "stdout", "stderr"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    quotes: b"\"'",
    multiline: b"",
    raw: b"",
    triple: false,
    capitalized_types: false,
    calls: true,
};

const CPP: Spec = Spec {
    flavour: Flavour::C,
    keywords: &[
        "alignas",
        "alignof",
        "and",
        "asm",
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "co_await",
        "co_return",
        "co_yield",
        "concept",
        "const",
        "const_cast",
        "consteval",
        "constexpr",
        "constinit",
        "continue",
        "decltype",
        "default",
        "delete",
        "do",
        "dynamic_cast",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "final",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "not",
        "operator",
        "or",
        "override",
        "private",
        "protected",
        "public",
        "register",
        "reinterpret_cast",
        "requires",
        "return",
        "sizeof",
        "static",
        "static_assert",
        "static_cast",
        "struct",
        "switch",
        "template",
        "this",
        "thread_local",
        "throw",
        "try",
        "typedef",
        "typeid",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
    ],
    types: C_TYPES,
    constants: &["NULL", "true", "false", "nullptr"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    quotes: b"\"'",
    multiline: b"",
    raw: b"",
    triple: false,
    capitalized_types: false,
    calls: true,
};

const GO: Spec = Spec {
    flavour: Flavour::Go,
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "type",
        "var",
    ],
    types: &[
        "any",
        "bool",
        "byte",
        "comparable",
        "complex64",
        "complex128",
        "error",
        "float32",
        "float64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "rune",
        "string",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
    ],
    constants: &["true", "false", "nil", "iota"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    quotes: b"\"'`",
    multiline: b"`",
    raw: b"`",
    triple: false,
    capitalized_types: false,
    calls: true,
};

const NIX: Spec = Spec {
    flavour: Flavour::Nix,
    keywords: &["assert", "else", "if", "in", "inherit", "let", "or", "rec", "then", "with", "import"],
    types: &[],
    constants: &["true", "false", "null"],
    line_comments: &["#"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    quotes: b"\"",
    multiline: b"\"",
    raw: b"",
    triple: false,
    capitalized_types: false,
    calls: false,
};

const SHELL: Spec = Spec {
    flavour: Flavour::Shell,
    keywords: &[
        "if", "then", "else", "elif", "fi", "case", "esac", "for", "while", "until", "do", "done", "in", "function",
        "select", "time", "return", "break", "continue", "local", "export", "readonly", "declare", "typeset", "unset",
        "shift", "exit", "source", "alias", "set", "trap", "eval", "exec",
    ],
    types: &[],
    constants: &["true", "false"],
    line_comments: &["#"],
    block_comment: None,
    nested_comments: false,
    quotes: b"\"'`",
    multiline: b"\"'`",
    raw: b"'",
    triple: false,
    capitalized_types: false,
    calls: false,
};

const TOML: Spec = Spec {
    flavour: Flavour::Toml,
    keywords: &[],
    types: &[],
    constants: &["true", "false", "inf", "nan"],
    line_comments: &["#"],
    block_comment: None,
    nested_comments: false,
    quotes: b"\"'",
    multiline: b"",
    raw: b"'",
    triple: true,
    capitalized_types: false,
    calls: false,
};

const JSON: Spec = Spec {
    flavour: Flavour::Json,
    keywords: &[],
    types: &[],
    constants: &["true", "false", "null"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    quotes: b"\"",
    multiline: b"",
    raw: b"",
    triple: false,
    capitalized_types: false,
    calls: false,
};

pub(crate) const YAML: Spec = Spec {
    flavour: Flavour::Yaml,
    keywords: &[],
    types: &[],
    constants: &[
        "true", "false", "null", "True", "False", "Null", "TRUE", "FALSE", "NULL", "yes", "no", "on", "off", "~",
    ],
    line_comments: &["#"],
    block_comment: None,
    nested_comments: false,
    quotes: b"\"'",
    multiline: b"",
    raw: b"'",
    triple: false,
    capitalized_types: false,
    calls: false,
};

pub fn spec(lang: Language) -> Option<&'static Spec> {
    Some(match lang {
        Language::Rust => &RUST,
        Language::Python => &PYTHON,
        Language::JavaScript => &JAVASCRIPT,
        Language::TypeScript => &TYPESCRIPT,
        Language::C => &C,
        Language::Cpp => &CPP,
        Language::Go => &GO,
        Language::Nix => &NIX,
        Language::Shell => &SHELL,
        Language::Toml => &TOML,
        Language::Json => &JSON,
        Language::Yaml => &YAML,
        Language::Plain | Language::Markdown => return None,
    })
}

#[inline]
fn ident_start(spec: &Spec, c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 || (c == b'$' && spec.flavour == Flavour::Js)
}

#[inline]
fn ident_char(spec: &Spec, c: u8) -> bool {
    c.is_ascii_alphanumeric()
        || c == b'_'
        || c >= 0x80
        || (c == b'$' && spec.flavour == Flavour::Js)
        || (matches!(spec.flavour, Flavour::Nix | Flavour::Toml | Flavour::Yaml)
            && (c == b'-' || (c == b'\'' && spec.flavour == Flavour::Nix)))
}

fn skip_spaces(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    i
}

/// The end of a number starting at `i`: digits, `0x…`, a fraction, an
/// exponent, `_` separators and a type suffix.
fn number_end(b: &[u8], mut i: usize) -> usize {
    let n = b.len();
    if b[i] == b'0' && i + 1 < n && matches!(b[i + 1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
        i += 2;
        while i < n && (b[i].is_ascii_hexdigit() || b[i] == b'_') {
            i += 1;
        }
    } else {
        while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
            i += 1;
        }
        if i + 1 < n && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            i += 1;
            while i < n && (b[i].is_ascii_digit() || b[i] == b'_') {
                i += 1;
            }
        }
        if i < n && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < n && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            if j < n && b[j].is_ascii_digit() {
                i = j;
                while i < n && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
    }
    // Suffixes: 10u32, 1.5f64, 100n, 10ULL.
    while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
        i += 1;
    }
    i
}

/// Scan a string's body from `i`: where it ends, and whether it closed.
fn string_end(spec: &Spec, b: &[u8], mut i: usize, quote: u8, triple: bool) -> (usize, bool) {
    let n = b.len();
    let escapes = !spec.raw.contains(&quote);
    while i < n {
        let c = b[i];
        if c == b'\\' && escapes {
            i += 2;
            continue;
        }
        if c == quote {
            if !triple {
                return (i + 1, true);
            }
            if i + 2 < n && b[i + 1] == quote && b[i + 2] == quote {
                return (i + 3, true);
            }
        }
        i += 1;
    }
    (n, false)
}

/// Scan a block comment from `i`, `depth` deep: where it ends and how deep
/// it still is there (0: closed).
fn comment_end(spec: &Spec, b: &[u8], mut i: usize, mut depth: u8) -> (usize, u8) {
    let Some((open, close)) = spec.block_comment else { return (b.len(), 0) };
    let (open, close) = (open.as_bytes(), close.as_bytes());
    while i < b.len() {
        if b[i..].starts_with(close) {
            i += close.len();
            depth -= 1;
            if depth == 0 {
                return (i, 0);
            }
        } else if spec.nested_comments && b[i..].starts_with(open) {
            i += open.len();
            depth = depth.saturating_add(1);
        } else {
            i += 1;
        }
    }
    (b.len(), depth)
}

/// Tokenize one line of code from `mode`; the mode the next line starts in.
pub fn lex(spec: &Spec, mut mode: Mode, line: &str, out: &mut Out) -> Mode {
    let b = line.as_bytes();
    let n = b.len();
    let mut i = 0;
    // Where the construct the mode stands for began on this line.
    let mut from = 0;
    // A shell line continued with `\` keeps a single-quoted string going, but
    // C's strings end with their line unless it ends in a backslash.
    loop {
        match mode {
            Mode::Normal => {}
            Mode::Comment { depth } => {
                let (end, d) = comment_end(spec, b, i, depth);
                out.push(from..end, Kind::Comment);
                i = end;
                if d > 0 {
                    return Mode::Comment { depth: d };
                }
            }
            Mode::Str { quote, triple } => {
                let (end, closed) = string_end(spec, b, i, quote, triple);
                out.push(from..end, Kind::String);
                i = end;
                if !closed {
                    let continues = triple || spec.multiline.contains(&quote) || (n > 0 && b[n - 1] == b'\\');
                    return if continues { mode } else { Mode::Normal };
                }
                // A quoted key: `"name": …`, `"name" = …`.
                if matches!(spec.flavour, Flavour::Json | Flavour::Yaml | Flavour::Toml) {
                    let j = skip_spaces(b, i);
                    let key = match spec.flavour {
                        Flavour::Toml => {
                            j < n && b[j] == b'=' && b[..from].iter().all(|c| matches!(c, b' ' | b'\t' | b'.'))
                        }
                        _ => j < n && b[j] == b':',
                    };
                    if key {
                        out.retag_last(Kind::String, Kind::Key);
                    }
                }
            }
            Mode::RawStr { hashes } => {
                let mut j = i;
                let mut closed = None;
                while j < n {
                    if b[j] == b'"'
                        && b[j + 1..].iter().take(hashes as usize).filter(|c| **c == b'#').count() == hashes as usize
                    {
                        closed = Some(j + 1 + hashes as usize);
                        break;
                    }
                    j += 1;
                }
                match closed {
                    Some(end) => {
                        out.push(from..end, Kind::String);
                        i = end;
                    }
                    None => {
                        out.push(from..n, Kind::String);
                        return mode;
                    }
                }
            }
            Mode::Indented => {
                // Nix's '' string: '' closes it, but ''' ''$ and ''\ escape.
                let mut j = i;
                let mut closed = None;
                while j + 1 < n {
                    if b[j] == b'\'' && b[j + 1] == b'\'' {
                        match b.get(j + 2) {
                            Some(b'\'') | Some(b'$') => j += 3,
                            Some(b'\\') => j += 4,
                            _ => {
                                closed = Some(j + 2);
                                break;
                            }
                        }
                    } else {
                        j += 1;
                    }
                }
                match closed {
                    Some(end) => {
                        out.push(from..end, Kind::String);
                        i = end;
                    }
                    None => {
                        out.push(from..n, Kind::String);
                        return mode;
                    }
                }
            }
        }
        // Normal code from `i` until something opens a mode again.
        let opened = normal(spec, b, line, &mut i, out);
        match opened {
            Some((m, start)) => {
                mode = m;
                from = start;
            }
            None => return Mode::Normal,
        }
    }
}

/// Scan normal code from `*i`: stops at the end of the line (None), or
/// when a comment or string opens (its mode, and where it began).
fn normal(spec: &Spec, b: &[u8], line: &str, i: &mut usize, out: &mut Out) -> Option<(Mode, usize)> {
    let n = b.len();
    let fl = spec.flavour;
    // Where the line's code starts (for keys and directives).
    let first = skip_spaces(b, 0);
    // The name before this one (`fn` makes the next a function).
    let mut prev: &str = "";
    while *i < n {
        let s = *i;
        let c = b[s];
        if c == b' ' || c == b'\t' {
            *i += 1;
            continue;
        }
        // Line comments; a shell or YAML `#` only at a word's start.
        if let Some(lc) = spec.line_comments.iter().find(|lc| b[s..].starts_with(lc.as_bytes())) {
            let word_start = s == 0 || matches!(b[s - 1], b' ' | b'\t' | b';' | b'(');
            if !(lc == &"#" && matches!(fl, Flavour::Shell | Flavour::Yaml) && !word_start) {
                // Rust's doc comments are comments too.
                out.push(s..n, Kind::Comment);
                *i = n;
                return None;
            }
        }
        if let Some((open, _)) = spec.block_comment
            && b[s..].starts_with(open.as_bytes())
        {
            *i = s + open.len();
            return Some((Mode::Comment { depth: 1 }, s));
        }
        // Strings.
        if spec.quotes.contains(&c) {
            if fl == Flavour::Rust && c == b'\'' {
                // handled below with lifetimes
            } else {
                if spec.triple && s + 2 < n && b[s + 1] == c && b[s + 2] == c {
                    *i = s + 3;
                    return Some((Mode::Str { quote: c, triple: true }, s));
                }
                *i = s + 1;
                return Some((Mode::Str { quote: c, triple: false }, s));
            }
        }
        match (fl, c) {
            (Flavour::Rust, b'\'') => {
                // A char ('x', '\n', 'é') or a lifetime ('a).
                let rest = &line[s + 1..];
                let mut chars = rest.char_indices();
                match (chars.next(), chars.next()) {
                    (Some((_, '\\')), _) => {
                        let (end, _) = string_end(spec, b, s + 1, b'\'', false);
                        out.push(s..end, Kind::String);
                        *i = end;
                    }
                    (Some((_, ch)), Some((k, '\''))) if ch != '\'' => {
                        out.push(s..s + 1 + k + 1, Kind::String);
                        *i = s + 1 + k + 1;
                    }
                    (Some((_, ch)), _) if ch.is_alphabetic() || ch == '_' => {
                        let mut j = s + 1;
                        while j < n && ident_char(spec, b[j]) {
                            j += 1;
                        }
                        out.push(s..j, Kind::Meta);
                        *i = j;
                    }
                    _ => *i = s + 1,
                }
                continue;
            }
            (Flavour::Rust, b'#')
                if b.get(s + 1) == Some(&b'[') || (b.get(s + 1) == Some(&b'!') && b.get(s + 2) == Some(&b'[')) =>
            {
                let end = if b[s + 1] == b'!' { s + 3 } else { s + 2 };
                out.push(s..end, Kind::Meta);
                *i = end;
                continue;
            }
            (Flavour::C, b'#') if s == first => {
                // A preprocessor line: `#include <stdio.h>`.
                let mut j = skip_spaces(b, s + 1);
                while j < n && b[j].is_ascii_alphabetic() {
                    j += 1;
                }
                out.push(s..j, Kind::Keyword);
                let word = &line[s + 1..j].trim();
                *i = j;
                if *word == "include" {
                    let k = skip_spaces(b, j);
                    if k < n
                        && b[k] == b'<'
                        && let Some(e) = line[k..].find('>')
                    {
                        out.push(k..k + e + 1, Kind::String);
                        *i = k + e + 1;
                    }
                }
                continue;
            }
            (Flavour::Shell, b'$') => {
                // $name, ${…}, $1, $@, $?
                let mut j = s + 1;
                if j < n && b[j] == b'{' {
                    j = line[j..].find('}').map_or(n, |e| j + e + 1);
                } else if j < n
                    && (b[j].is_ascii_digit() || matches!(b[j], b'@' | b'?' | b'#' | b'*' | b'$' | b'!' | b'-'))
                {
                    j += 1;
                } else {
                    while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                        j += 1;
                    }
                }
                if j > s + 1 {
                    out.push(s..j, Kind::Variable);
                }
                *i = j.max(s + 1);
                continue;
            }
            (Flavour::Nix, b'\'') if b.get(s + 1) == Some(&b'\'') => {
                *i = s + 2;
                return Some((Mode::Indented, s));
            }
            (Flavour::Toml, b'[') if s == first => {
                let end = line[s..].find(']').map_or(n, |e| {
                    let e = s + e + 1;
                    if b.get(e) == Some(&b']') { e + 1 } else { e }
                });
                out.push(s..end, Kind::Section);
                *i = end;
                continue;
            }
            (Flavour::Yaml, b'&' | b'*') if s + 1 < n && ident_start(spec, b[s + 1]) => {
                let mut j = s + 1;
                while j < n && ident_char(spec, b[j]) {
                    j += 1;
                }
                out.push(s..j, Kind::Variable);
                *i = j;
                continue;
            }
            (Flavour::Yaml, b'!') => {
                let mut j = s + 1;
                while j < n && !matches!(b[j], b' ' | b'\t') {
                    j += 1;
                }
                out.push(s..j, Kind::Meta);
                *i = j;
                continue;
            }
            (Flavour::Yaml, b'-' | b'.') if s == 0 && (line == "---" || line == "..." || line.starts_with("--- ")) => {
                out.push(0..3, Kind::Meta);
                *i = 3;
                continue;
            }
            _ => {}
        }
        if c.is_ascii_digit() {
            // A number, unless it's a YAML/TOML key or date-ish word.
            let end = number_end(b, s);
            out.push(s..end, Kind::Number);
            *i = end;
            continue;
        }
        if ident_start(spec, c) {
            let mut j = s + 1;
            while j < n && ident_char(spec, b[j]) {
                j += 1;
            }
            // Nix and TOML names don't end in '-' they're followed by.
            let word = &line[s..j];
            // String prefixes: Rust's r"…", r#"…"#, b"…"; Python's f"…", rb'…'.
            if fl == Flavour::Rust && matches!(word, "r" | "br" | "cr") && j < n && (b[j] == b'"' || b[j] == b'#') {
                let mut k = j;
                while k < n && b[k] == b'#' {
                    k += 1;
                }
                if k < n && b[k] == b'"' {
                    *i = k + 1;
                    return Some((Mode::RawStr { hashes: (k - j) as u8 }, s));
                }
            }
            if fl == Flavour::Rust && matches!(word, "b" | "c") && j < n && b[j] == b'"' {
                *i = j + 1;
                return Some((Mode::Str { quote: b'"', triple: false }, s));
            }
            if fl == Flavour::Rust && word == "b" && j < n && b[j] == b'\'' {
                let (end, _) = string_end(spec, b, j + 1, b'\'', false);
                out.push(s..end, Kind::String);
                *i = end;
                continue;
            }
            if fl == Flavour::Python
                && j < n
                && (b[j] == b'"' || b[j] == b'\'')
                && word.len() <= 2
                && word.chars().all(|ch| matches!(ch.to_ascii_lowercase(), 'r' | 'b' | 'f' | 'u'))
            {
                let q = b[j];
                if j + 2 < n && b[j + 1] == q && b[j + 2] == q {
                    *i = j + 3;
                    return Some((Mode::Str { quote: q, triple: true }, s));
                }
                *i = j + 1;
                return Some((Mode::Str { quote: q, triple: false }, s));
            }
            let kind = if matches!(prev, "fn" | "def" | "func" | "function") && !spec.keywords.contains(&word) {
                Some(Kind::Function)
            } else {
                classify(spec, b, line, s, j, first)
            };
            prev = word;
            if let Some(k) = kind {
                out.push(s..j, k);
            }
            *i = j;
            continue;
        }
        // Anything else: punctuation, one character at a time.
        *i = s + utf8_len(c);
    }
    None
}

fn utf8_len(c: u8) -> usize {
    match c {
        0xf0.. => 4,
        0xe0.. => 3,
        0xc0.. => 2,
        _ => 1,
    }
}

/// What the name at `s..j` is.
fn classify(spec: &Spec, b: &[u8], line: &str, s: usize, j: usize, first: usize) -> Option<Kind> {
    let word = &line[s..j];
    let n = b.len();
    let next = skip_spaces(b, j);
    let fl = spec.flavour;
    // Keys: `key:` (YAML), `key =` (TOML, Nix).
    match fl {
        Flavour::Yaml => {
            let at_start = s == first || line[first..s].trim_start_matches(['-', ' ']).is_empty();
            if at_start && next < n && b[next] == b':' && (next + 1 == n || matches!(b[next + 1], b' ' | b'\t')) {
                return Some(Kind::Key);
            }
            // Values: words after a key are plain text, but constants count.
            return spec.constants.contains(&word).then_some(Kind::Constant);
        }
        Flavour::Toml => {
            if next < n
                && (b[next] == b'=' || b[next] == b'.')
                && line[..s].trim_start().chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '"'))
            {
                return Some(Kind::Key);
            }
        }
        Flavour::Nix => {
            if next < n && b[next] == b'=' && b.get(next + 1) != Some(&b'=') {
                return Some(Kind::Key);
            }
            if next < n && b[next] == b'.' && b.get(next + 1).is_some_and(|c| ident_start(spec, *c)) {
                // a.b.c = …: the path's parts are keys if it ends in '='.
                let rest = &line[next..];
                let path_end = rest
                    .find(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-' | '\'')))
                    .unwrap_or(rest.len());
                let after = skip_spaces(b, next + path_end);
                if after < n && b[after] == b'=' && b.get(after + 1) != Some(&b'=') {
                    return Some(Kind::Key);
                }
            }
        }
        _ => {}
    }
    if spec.keywords.contains(&word) {
        return Some(Kind::Keyword);
    }
    if spec.constants.contains(&word) {
        return Some(Kind::Constant);
    }
    if spec.types.contains(&word) {
        return Some(Kind::Type);
    }
    // Rust's macros: println!, vec![…].
    if fl == Flavour::Rust && j < n && b[j] == b'!' && b.get(j + 1) != Some(&b'=') {
        return Some(Kind::Function);
    }
    let upper = word.as_bytes()[0].is_ascii_uppercase();
    if upper && word.len() > 1 && word.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') {
        return Some(Kind::Constant);
    }
    if spec.capitalized_types && upper {
        return Some(Kind::Type);
    }
    if spec.calls
        && next < n
        && (b[next] == b'(' || (fl == Flavour::Rust && b[next] == b':' && line[next..].starts_with("::<")))
    {
        return Some(Kind::Function);
    }
    // Go's and C's declarations: `func name(`, handled by the call rule.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{LineState, Token, tokens};

    fn kinds(lang: Language, line: &str) -> Vec<(String, Kind)> {
        let (t, _) = tokens(lang, LineState::default(), line, 1);
        t.into_iter().map(|Token { range, kind }| (line[range].to_owned(), kind)).collect()
    }

    fn has(lang: Language, line: &str, text: &str, kind: Kind) -> bool {
        kinds(lang, line).iter().any(|(t, k)| t == text && *k == kind)
    }

    #[test]
    fn rust() {
        let l =
            r#"pub fn parse<'a>(s: &'a str) -> Result<u32, Error> { let x = 0x1F_u32; println!("{x} \"q\""); 'c' }"#;
        assert!(has(Language::Rust, l, "pub", Kind::Keyword));
        assert!(has(Language::Rust, l, "fn", Kind::Keyword));
        assert!(has(Language::Rust, l, "parse", Kind::Function));
        assert!(has(Language::Rust, l, "'a", Kind::Meta));
        assert!(has(Language::Rust, l, "str", Kind::Type));
        assert!(has(Language::Rust, l, "Result", Kind::Type));
        assert!(has(Language::Rust, l, "0x1F_u32", Kind::Number));
        assert!(has(Language::Rust, l, "println", Kind::Function));
        assert!(has(Language::Rust, l, r#""{x} \"q\"""#, Kind::String));
        assert!(has(Language::Rust, l, "'c'", Kind::String));
        assert!(has(Language::Rust, "let s = r#\"a \"quoted\" b\"#;", "r#\"a \"quoted\" b\"#", Kind::String));
        assert!(has(Language::Rust, "#[derive(Debug)] const MAX: usize = 1;", "#[", Kind::Meta));
        assert!(has(Language::Rust, "#[derive(Debug)] const MAX: usize = 1;", "MAX", Kind::Constant));
        assert!(has(Language::Rust, "x // note", "// note", Kind::Comment));
    }

    #[test]
    fn block_comments_and_strings_run_over_lines() {
        let (_, s) = tokens(Language::Rust, LineState::default(), "let a = 1; /* one /* two */", 0);
        assert_eq!(s.mode, Mode::Comment { depth: 1 });
        let (t, s) = tokens(Language::Rust, s, "still */ let b = \"open", 1);
        assert_eq!(t[0].range, 0..8);
        assert_eq!(t[0].kind, Kind::Comment);
        assert_eq!(s.mode, Mode::Str { quote: b'"', triple: false });
        let (t, s) = tokens(Language::Rust, s, "closed\"; 2", 2);
        assert_eq!((t[0].range.clone(), t[0].kind), (0..7, Kind::String));
        assert_eq!(s.mode, Mode::Normal);
        // C's strings end with their line.
        let (_, s) = tokens(Language::C, LineState::default(), "char *s = \"open", 0);
        assert_eq!(s.mode, Mode::Normal);
        // Python's triple quotes.
        let (_, s) = tokens(Language::Python, LineState::default(), "doc = \"\"\"Start", 0);
        assert_eq!(s.mode, Mode::Str { quote: b'"', triple: true });
        let (_, s) = tokens(Language::Python, s, "end\"\"\" # done", 1);
        assert_eq!(s.mode, Mode::Normal);
    }

    #[test]
    fn python_js_go_c() {
        assert!(has(Language::Python, "def f(x): return None  # why", "def", Kind::Keyword));
        assert!(has(Language::Python, "def f(x): return None  # why", "None", Kind::Constant));
        assert!(has(Language::Python, "def f(x): return None  # why", "# why", Kind::Comment));
        assert!(has(Language::Python, "s = f'{x}'", "f'{x}'", Kind::String));
        assert!(has(Language::JavaScript, "const x = `a ${b}` // c", "const", Kind::Keyword));
        assert!(has(Language::JavaScript, "const x = `a ${b}` // c", "`a ${b}`", Kind::String));
        assert!(has(Language::TypeScript, "interface A { n: number }", "interface", Kind::Keyword));
        assert!(has(Language::TypeScript, "interface A { n: number }", "number", Kind::Type));
        assert!(has(Language::Go, "func main() { var x int = 3 }", "func", Kind::Keyword));
        assert!(has(Language::Go, "func main() { var x int = 3 }", "main", Kind::Function));
        assert!(has(Language::Go, "func main() { var x int = 3 }", "int", Kind::Type));
        assert!(has(Language::C, "#include <stdio.h>", "#include", Kind::Keyword));
        assert!(has(Language::C, "#include <stdio.h>", "<stdio.h>", Kind::String));
        assert!(has(Language::Cpp, "template <typename T> class V {};", "template", Kind::Keyword));
    }

    #[test]
    fn nix_shell_toml_json_yaml() {
        let l = "  services.foo.enable = true; # on";
        assert!(has(Language::Nix, l, "services", Kind::Key));
        assert!(has(Language::Nix, l, "true", Kind::Constant));
        assert!(has(Language::Nix, l, "# on", Kind::Comment));
        let (_, s) = tokens(Language::Nix, LineState::default(), "script = ''", 0);
        assert_eq!(s.mode, Mode::Indented);
        let (t, s) = tokens(Language::Nix, s, "  echo ''${x} '';", 1);
        assert_eq!(s.mode, Mode::Normal);
        assert_eq!(t[0].kind, Kind::String);
        assert!(has(Language::Shell, "echo \"$HOME\" $1 # x", "$1", Kind::Variable));
        assert!(has(Language::Shell, "if [ -f a#b ]; then", "if", Kind::Keyword));
        assert!(!kinds(Language::Shell, "x=a#b").iter().any(|(_, k)| *k == Kind::Comment));
        assert!(has(Language::Toml, "[package.metadata]", "[package.metadata]", Kind::Section));
        assert!(has(Language::Toml, "name = \"sumi\"", "name", Kind::Key));
        assert!(has(Language::Toml, "name = \"sumi\"", "\"sumi\"", Kind::String));
        assert!(has(Language::Json, "  \"key\": [1, 2.5e3, true, null]", "\"key\"", Kind::Key));
        assert!(has(Language::Json, "  \"key\": [1, 2.5e3, true, null]", "2.5e3", Kind::Number));
        assert!(has(Language::Yaml, "- name: build # step", "name", Kind::Key));
        assert!(has(Language::Yaml, "- name: build # step", "# step", Kind::Comment));
        assert!(has(Language::Yaml, "enabled: true", "true", Kind::Constant));
    }
}
