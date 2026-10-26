//! Syntax colouring: a small tokenizer per language (keywords, strings,
//! comments, numbers, types), Markdown styled as it's typed (with fenced
//! code coloured in its own language), and a cache of each line's starting
//! state so that an edit re-tokenizes only the lines it touched, plus those
//! after it whose state it carried over.

pub mod cache;
pub mod code;
pub mod markdown;

use std::ops::Range;
use std::path::Path;

use aizome_kit::{LineStyle, Role, Span};

pub use cache::Highlight;

/// What a file is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    Plain,
    Markdown,
    Rust,
    Python,
    JavaScript,
    TypeScript,
    C,
    Cpp,
    Go,
    Nix,
    Shell,
    Toml,
    Json,
    Yaml,
}

impl Language {
    /// In the order the language pop-up lists them.
    pub const ALL: [Language; 14] = [
        Language::Plain,
        Language::Markdown,
        Language::C,
        Language::Cpp,
        Language::Go,
        Language::JavaScript,
        Language::Json,
        Language::Nix,
        Language::Python,
        Language::Rust,
        Language::Shell,
        Language::Toml,
        Language::TypeScript,
        Language::Yaml,
    ];

    /// Its name in the pop-up and the menus.
    pub fn name(self) -> &'static str {
        match self {
            Language::Plain => "Plain Text",
            Language::Markdown => "Markdown",
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::Go => "Go",
            Language::Nix => "Nix",
            Language::Shell => "Shell",
            Language::Toml => "TOML",
            Language::Json => "JSON",
            Language::Yaml => "YAML",
        }
    }

    /// A stable name, as kept in the session.
    pub fn id(self) -> &'static str {
        match self {
            Language::Plain => "plain",
            Language::Markdown => "markdown",
            Language::Rust => "rust",
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Go => "go",
            Language::Nix => "nix",
            Language::Shell => "shell",
            Language::Toml => "toml",
            Language::Json => "json",
            Language::Yaml => "yaml",
        }
    }

    pub fn index(self) -> usize {
        Language::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    /// The language a Markdown fence's info string (or an id) names.
    pub fn from_name(name: &str) -> Option<Language> {
        let n = name.trim().trim_start_matches('.').to_ascii_lowercase();
        Some(match n.as_str() {
            "plain" | "text" | "txt" | "plaintext" => Language::Plain,
            "markdown" | "md" | "mdown" | "mkd" => Language::Markdown,
            "rust" | "rs" => Language::Rust,
            "python" | "py" | "python3" | "py3" | "pyi" => Language::Python,
            "javascript" | "js" | "jsx" | "mjs" | "cjs" | "node" => Language::JavaScript,
            "typescript" | "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
            "c" | "h" => Language::C,
            "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "h++" => Language::Cpp,
            "go" | "golang" => Language::Go,
            "nix" => Language::Nix,
            "shell" | "sh" | "bash" | "zsh" | "ksh" | "console" | "shellsession" | "fish" => Language::Shell,
            "toml" => Language::Toml,
            "json" | "jsonc" | "json5" | "geojson" => Language::Json,
            "yaml" | "yml" => Language::Yaml,
            _ => return None,
        })
    }

    /// The language of a file, by its name, else by its first line (a
    /// `#!` line, a modeline's worth of hints).
    pub fn detect(path: Option<&Path>, first_line: &str) -> Language {
        if let Some(p) = path {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
            match name.as_str() {
                "cargo.lock" | "poetry.lock" | "uv.lock" => return Language::Toml,
                "flake.lock" | "package-lock.json" | ".eslintrc" | "tsconfig.json" => return Language::Json,
                ".bashrc" | ".bash_profile" | ".profile" | ".zshrc" | ".zprofile" | ".envrc" | "pkgbuild" => {
                    return Language::Shell;
                }
                "readme" | "changelog" => return Language::Markdown,
                _ => {}
            }
            if let Some(ext) = p.extension().and_then(|e| e.to_str())
                && let Some(l) = Language::from_name(ext)
            {
                return l;
            }
        }
        let first = first_line.trim();
        if let Some(bang) = first.strip_prefix("#!") {
            // `#!/usr/bin/env python3`, `#!/bin/bash`, `#! /usr/bin/env -S nix-shell`.
            let words: Vec<&str> = bang.split_whitespace().collect();
            let prog = match words.first().map(|w| w.rsplit('/').next().unwrap_or(w)) {
                Some("env") => words.iter().skip(1).find(|w| !w.starts_with('-')).copied().unwrap_or(""),
                Some(p) => p,
                None => "",
            };
            let prog = prog.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
            return match prog {
                "python" => Language::Python,
                "node" | "deno" | "bun" => Language::JavaScript,
                "nix-shell" => Language::Shell,
                "sh" | "bash" | "zsh" | "dash" | "ksh" | "fish" => Language::Shell,
                _ => Language::Plain,
            };
        }
        Language::Plain
    }

    /// The tokenizer's rules (none for Plain Text and Markdown).
    pub fn spec(self) -> Option<&'static code::Spec> {
        code::spec(self)
    }
}

/// What a run of text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Keyword,
    Type,
    String,
    Comment,
    Number,
    /// `true`, `None`, `null`, ALL_CAPS.
    Constant,
    /// A name called: `parse(`, `println!`.
    Function,
    /// Attributes, preprocessor lines, lifetimes, YAML tags.
    Meta,
    /// A key in JSON, YAML, TOML or a Nix attribute set.
    Key,
    /// `$name` in shell.
    Variable,
    /// A TOML `[table]`.
    Section,
    // Markdown.
    Heading,
    /// The `#`s of a heading.
    HeadingMark,
    Code,
    Emphasis,
    Strong,
    StrongEmphasis,
    /// `*`, `_`, `~~` around emphasis.
    EmphasisMark,
    Strikethrough,
    LinkText,
    Url,
    Bullet,
    /// `[ ]` and `[x]`.
    TaskBox,
    /// The text of a ticked task.
    TaskDone,
    QuoteMark,
    Quote,
    Rule,
    /// ``` and ~~~ lines, and front matter's `---`.
    FenceMark,
    /// A fence's language.
    FenceInfo,
    TablePipe,
    Html,
}

/// A run of a line: bytes `range` are `kind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub range: Range<usize>,
    pub kind: Kind,
}

/// Where tokens go, if anywhere: working out a line's end state needs no
/// tokens.
pub struct Out<'a>(pub Option<&'a mut Vec<Token>>);

impl Out<'_> {
    #[inline]
    pub fn push(&mut self, range: Range<usize>, kind: Kind) {
        if let Some(v) = &mut self.0
            && range.start < range.end
        {
            v.push(Token { range, kind });
        }
    }

    pub fn wants(&self) -> bool {
        self.0.is_some()
    }

    /// Re-kind the last token if it's `from` (a string that turns out to be
    /// a key).
    pub fn retag_last(&mut self, from: Kind, to: Kind) {
        if let Some(v) = &mut self.0
            && let Some(t) = v.last_mut()
            && t.kind == from
        {
            t.kind = to;
        }
    }
}

/// What a code tokenizer carries from one line to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Normal,
    /// In a block comment, this deep (Rust's nest).
    Comment { depth: u8 },
    /// In a string opened by `quote` (three of them if `triple`).
    Str { quote: u8, triple: bool },
    /// In a Rust raw string with this many `#`s.
    RawStr { hashes: u8 },
    /// In a Nix `''` string.
    Indented,
}

/// A Markdown block that runs over lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Block {
    #[default]
    None,
    /// In fenced code: its language, the fence's character and length.
    Fence { lang: Language, ch: u8, len: u8 },
    /// In YAML front matter (`---` on the first line).
    FrontMatter,
    /// In an HTML comment.
    Html,
}

/// The state at the start of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LineState {
    pub block: Block,
    pub mode: Mode,
}

/// Tokenize `line` (line `number`, without its line break) of a `lang`
/// file, starting in `state`: its tokens go to `out`, and the state the
/// next line starts in comes back.
pub fn tokenize(lang: Language, state: LineState, line: &str, number: usize, out: &mut Out) -> LineState {
    match lang {
        Language::Plain => LineState::default(),
        Language::Markdown => markdown::line(state, line, number == 0, out),
        _ => match lang.spec() {
            Some(spec) => LineState { block: Block::None, mode: code::lex(spec, state.mode, line, out) },
            None => LineState::default(),
        },
    }
}

/// A line's tokens.
pub fn tokens(lang: Language, state: LineState, line: &str, number: usize) -> (Vec<Token>, LineState) {
    let mut v = Vec::new();
    let end = tokenize(lang, state, line, number, &mut Out(Some(&mut v)));
    (v, end)
}

/// How a kind of text looks, in the theme's roles: violet keywords, green
/// strings, faint comments, amber numbers, cyan types; Markdown's headings
/// in blue 600 (their `#`s lighter), code green, links cyan, bullets violet.
pub fn span(kind: Kind) -> Span {
    let s = Span::new("");
    match kind {
        Kind::Keyword => s.role(Role::Violet),
        Kind::Type => s.role(Role::Cyan),
        Kind::String => s.role(Role::Success),
        Kind::Comment => s.role(Role::Faint).italic(),
        Kind::Number | Kind::Constant => s.role(Role::Warning),
        Kind::Function => s.role(Role::Accent),
        Kind::Meta => s.role(Role::Cyan),
        Kind::Key => s.role(Role::Accent),
        Kind::Variable => s.role(Role::Cyan),
        Kind::Section => s.role(Role::Accent).weight(600.0),
        Kind::Heading => s.role(Role::Accent).weight(600.0),
        Kind::HeadingMark => s.role(Role::Accent).weight(600.0).alpha(0.55),
        Kind::Code => s.role(Role::Success),
        Kind::Emphasis => s.italic(),
        Kind::Strong => s.weight(700.0),
        Kind::StrongEmphasis => s.weight(700.0).italic(),
        Kind::EmphasisMark => s.role(Role::Faint),
        Kind::Strikethrough => s.strikethrough(),
        Kind::LinkText => s.role(Role::Cyan),
        Kind::Url => s.role(Role::Faint),
        Kind::Bullet | Kind::TaskBox | Kind::QuoteMark => s.role(Role::Violet),
        Kind::TaskDone => s.role(Role::Text3).strikethrough(),
        Kind::Quote => s.role(Role::Text3).italic(),
        Kind::Rule | Kind::FenceMark | Kind::TablePipe => s.role(Role::Faint),
        Kind::FenceInfo => s.role(Role::Cyan),
        Kind::Html => s.role(Role::Faint).italic(),
    }
}

/// Tokens as the editor's styles.
pub fn line_styles(tokens: &[Token]) -> Vec<LineStyle> {
    tokens.iter().map(|t| LineStyle { range: t.range.clone(), span: span(t.kind) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn languages_from_names_and_first_lines() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(Language::detect(Some(&p("/x/architecture.md")), ""), Language::Markdown);
        assert_eq!(Language::detect(Some(&p("main.rs")), ""), Language::Rust);
        assert_eq!(Language::detect(Some(&p("flake.nix")), ""), Language::Nix);
        assert_eq!(Language::detect(Some(&p("Cargo.lock")), ""), Language::Toml);
        assert_eq!(Language::detect(Some(&p("flake.lock")), ""), Language::Json);
        assert_eq!(Language::detect(Some(&p("x.hpp")), ""), Language::Cpp);
        assert_eq!(Language::detect(Some(&p("run")), "#!/usr/bin/env python3"), Language::Python);
        assert_eq!(Language::detect(Some(&p("run")), "#!/bin/bash -e"), Language::Shell);
        assert_eq!(Language::detect(None, "hello"), Language::Plain);
        for l in Language::ALL {
            assert_eq!(Language::from_name(l.id()), Some(l), "{l:?}");
        }
    }
}
