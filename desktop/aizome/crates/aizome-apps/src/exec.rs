//! `Exec` command lines, as the Desktop Entry Specification 1.5 defines
//! them ("The Exec key"): quoting, then field codes.
//!
//! The value reaching [`Exec::parse`] has already had the general string
//! escapes applied (so a file's `\\\\` is a `\\` here). Then:
//!
//! - Arguments are separated by spaces (or tabs, newlines). An argument
//!   with reserved characters is written in double quotes, inside which
//!   `\"`, `` \` ``, `\$` and `\\` stand for the character itself.
//! - For compatibility with GLib (which hands the line to a shell parser),
//!   a backslash outside quotes escapes the next character and single
//!   quotes quote literally. Neither is in the specification; both appear
//!   in real files.
//! - Field codes: `%f` one file, `%F` files, `%u` one URL, `%U` URLs, `%i`
//!   `--icon <Icon>`, `%c` the name, `%k` the desktop file's location, `%%`
//!   a percent sign. The deprecated `%d %D %n %N %v %m` are removed. Any
//!   other code makes the line invalid ([`Error::InvalidArgument`]), as
//!   does an unterminated quote or a program name containing `=`.
//! - An unquoted argument that is only a field code expanding to nothing
//!   (`%f` without files, `%i` without an icon) disappears; `%F`, `%U` and
//!   `%i` standing alone expand to as many arguments as they need; inside
//!   a longer argument they act like `%f`, `%u` and the icon name. Field
//!   codes inside quotes are expanded too (the specification leaves them
//!   undefined; this is GLib's behaviour).
//!
//! Targets (what to open) are absolute paths or URIs; relative paths are
//! made absolute against the current directory. `%f`/`%F` turn `file://`
//! URIs into paths and skip other URIs; `%u`/`%U` pass paths and URIs as
//! given (the specification allows either for local files).

use crate::{Error, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Code {
    File,
    Files,
    Url,
    Urls,
    Icon,
    Name,
    Location,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    Code(Code),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Arg {
    pieces: Vec<Piece>,
    quoted: bool,
}

impl Arg {
    fn push_char(&mut self, c: char) {
        match self.pieces.last_mut() {
            Some(Piece::Text(text)) => text.push(c),
            _ => self.pieces.push(Piece::Text(c.to_string())),
        }
    }

    fn standalone_code(&self) -> Option<Code> {
        match self.pieces.as_slice() {
            [Piece::Code(code)] if !self.quoted => Some(*code),
            _ => None,
        }
    }
}

/// What field codes expand to besides the targets: the entry's `%c`, `%i`
/// and `%k`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecContext<'a> {
    /// The localised `Name`, for `%c`.
    pub name: &'a str,
    /// The `Icon` value, for `%i`.
    pub icon: Option<&'a str>,
    /// The desktop file's path, for `%k`.
    pub location: Option<&'a Path>,
}

/// A parsed `Exec` command line, ready to expand into argument vectors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exec {
    args: Vec<Arg>,
}

impl Exec {
    /// Parse an `Exec` value (escapes already applied).
    pub fn parse(exec: &str) -> Result<Exec> {
        let invalid = |what: &str| Error::InvalidArgument(format!("invalid Exec line ({what}): {exec}"));
        #[derive(PartialEq)]
        enum State {
            Plain,
            Double,
            Single,
        }
        let mut args: Vec<Arg> = Vec::new();
        let mut current: Option<Arg> = None;
        let mut state = State::Plain;
        let mut chars = exec.chars();
        while let Some(c) = chars.next() {
            match (&state, c) {
                (State::Plain, ' ' | '\t' | '\n' | '\r') => {
                    if let Some(arg) = current.take() {
                        args.push(arg);
                    }
                }
                (State::Plain, '"') => {
                    current.get_or_insert_default().quoted = true;
                    state = State::Double;
                }
                (State::Plain, '\'') => {
                    current.get_or_insert_default().quoted = true;
                    state = State::Single;
                }
                (State::Plain, '\\') => match chars.next() {
                    Some(next) => current.get_or_insert_default().push_char(next),
                    None => return Err(invalid("trailing backslash")),
                },
                (State::Double, '"') | (State::Single, '\'') => state = State::Plain,
                (State::Double, '\\') => match chars.next() {
                    Some(next @ ('"' | '`' | '$' | '\\')) => current.get_or_insert_default().push_char(next),
                    Some(next) => {
                        let arg = current.get_or_insert_default();
                        arg.push_char('\\');
                        arg.push_char(next);
                    }
                    None => return Err(invalid("unterminated quote")),
                },
                (_, '%') => {
                    let arg = current.get_or_insert_default();
                    let code = match chars.next() {
                        Some('%') => {
                            arg.push_char('%');
                            continue;
                        }
                        Some('f') => Code::File,
                        Some('F') => Code::Files,
                        Some('u') => Code::Url,
                        Some('U') => Code::Urls,
                        Some('i') => Code::Icon,
                        Some('c') => Code::Name,
                        Some('k') => Code::Location,
                        Some('d' | 'D' | 'n' | 'N' | 'v' | 'm') => continue,
                        Some(other) => return Err(invalid(&format!("unknown field code %{other}"))),
                        None => return Err(invalid("lone % at the end")),
                    };
                    arg.pieces.push(Piece::Code(code));
                }
                (_, c) => current.get_or_insert_default().push_char(c),
            }
        }
        if state != State::Plain {
            return Err(invalid("unterminated quote"));
        }
        if let Some(arg) = current.take() {
            args.push(arg);
        }
        // Arguments left empty by deprecated codes disappear.
        args.retain(|arg| arg.quoted || !arg.pieces.is_empty());
        match args.first().map(|arg| arg.pieces.as_slice()) {
            None => return Err(invalid("no program")),
            Some([Piece::Text(program)]) if !program.is_empty() && !program.contains('=') => {}
            Some(_) => return Err(invalid("the program must be a plain name or path")),
        }
        Ok(Exec { args })
    }

    /// The program: the first argument (a name to look up in `PATH`, or a
    /// path).
    pub fn program(&self) -> &str {
        match self.args.first().map(|arg| arg.pieces.as_slice()) {
            Some([Piece::Text(program)]) => program,
            _ => "",
        }
    }

    fn codes(&self) -> impl Iterator<Item = Code> + '_ {
        self.args.iter().flat_map(|arg| arg.pieces.iter()).filter_map(|piece| match piece {
            Piece::Code(code) => Some(*code),
            Piece::Text(_) => None,
        })
    }

    /// Whether the command takes files or URLs (`%f`, `%F`, `%u` or `%U`).
    pub fn takes_targets(&self) -> bool {
        self.codes().any(|code| matches!(code, Code::File | Code::Files | Code::Url | Code::Urls))
    }

    /// Whether the command takes URLs (`%u` or `%U`), rather than only
    /// local files.
    pub fn takes_urls(&self) -> bool {
        self.codes().any(|code| matches!(code, Code::Url | Code::Urls))
    }

    /// Whether one invocation can take several targets (a standalone `%F`
    /// or `%U`).
    pub fn takes_many(&self) -> bool {
        self.args.iter().any(|arg| matches!(arg.standalone_code(), Some(Code::Files | Code::Urls)))
    }

    /// One argument vector, for at most one invocation: `%f`/`%u` use the
    /// first usable target, `%F`/`%U` all of them.
    pub fn expand(&self, targets: &[String], context: &ExecContext) -> Vec<String> {
        let resolved: Vec<Target> = targets.iter().map(|t| Target::new(t)).collect();
        self.expand_resolved(&resolved, context)
    }

    /// The argument vectors to run to open `targets`: one, unless the
    /// command takes a single file or URL (`%f`/`%u`) and there are
    /// several, in which case one per target, as the specification
    /// suggests. Targets the command can't take (non-`file:` URIs for
    /// `%f`) are skipped; with none left (or none given) it's one
    /// invocation without them.
    pub fn expand_each(&self, targets: &[String], context: &ExecContext) -> Vec<Vec<String>> {
        let takes_urls = self.takes_urls();
        let usable: Vec<Target> =
            targets.iter().map(|t| Target::new(t)).filter(|t| takes_urls || t.path().is_some()).collect();
        if !self.takes_targets() || self.takes_many() || usable.len() <= 1 {
            return vec![self.expand_resolved(&usable, context)];
        }
        usable.chunks(1).map(|one| self.expand_resolved(one, context)).collect()
    }

    fn expand_resolved(&self, targets: &[Target], context: &ExecContext) -> Vec<String> {
        let files = || targets.iter().filter_map(Target::path);
        let urls = || targets.iter().map(Target::as_given);
        let location = context.location.map(|p| p.to_string_lossy().into_owned());
        let icon = context.icon.filter(|icon| !icon.is_empty());
        let mut argv = Vec::new();
        for arg in &self.args {
            match arg.standalone_code() {
                Some(Code::Files) => argv.extend(files()),
                Some(Code::Urls) => argv.extend(urls()),
                Some(Code::File) => argv.extend(files().next()),
                Some(Code::Url) => argv.extend(urls().next()),
                Some(Code::Icon) => {
                    if let Some(icon) = icon {
                        argv.push("--icon".to_owned());
                        argv.push(icon.to_owned());
                    }
                }
                Some(Code::Location) => argv.extend(location.clone().filter(|l| !l.is_empty())),
                Some(Code::Name) => argv.push(context.name.to_owned()),
                None => {
                    let mut out = String::new();
                    for piece in &arg.pieces {
                        match piece {
                            Piece::Text(text) => out.push_str(text),
                            Piece::Code(Code::File | Code::Files) => out.extend(files().next()),
                            Piece::Code(Code::Url | Code::Urls) => out.extend(urls().next()),
                            Piece::Code(Code::Icon) => out.push_str(icon.unwrap_or("")),
                            Piece::Code(Code::Name) => out.push_str(context.name),
                            Piece::Code(Code::Location) => out.push_str(location.as_deref().unwrap_or("")),
                        }
                    }
                    argv.push(out);
                }
            }
        }
        argv
    }
}

/// Something to open: a local path or a URI.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Path(String),
    Uri { uri: String, path: Option<String> },
}

impl Target {
    fn new(target: &str) -> Target {
        if !target.starts_with('/') && has_scheme(target) {
            let path = file_uri_to_path(target).map(|p| p.to_string_lossy().into_owned());
            return Target::Uri { uri: target.to_owned(), path };
        }
        let path = Path::new(target);
        if path.is_absolute() {
            return Target::Path(target.to_owned());
        }
        match std::env::current_dir() {
            Ok(dir) => Target::Path(dir.join(path).to_string_lossy().into_owned()),
            Err(_) => Target::Path(target.to_owned()),
        }
    }

    /// The local path, for `%f`/`%F`.
    fn path(&self) -> Option<String> {
        match self {
            Target::Path(path) => Some(path.clone()),
            Target::Uri { path, .. } => path.clone(),
        }
    }

    /// As given (paths stay paths), for `%u`/`%U`.
    fn as_given(&self) -> String {
        match self {
            Target::Path(path) => path.clone(),
            Target::Uri { uri, .. } => uri.clone(),
        }
    }
}

/// Whether `s` starts with a URI scheme (`scheme:`), RFC 3986 style.
fn has_scheme(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The local path of a `file:` URI (`file:///p`, `file://localhost/p`,
/// `file:/p`), percent-decoded; `None` for other schemes and remote hosts.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let (scheme, rest) = uri.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let path = match rest.strip_prefix("//") {
        Some(authority_and_path) => {
            let slash = authority_and_path.find('/')?;
            let (host, path) = authority_and_path.split_at(slash);
            if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
                return None;
            }
            path
        }
        None if rest.starts_with('/') => rest,
        None => return None,
    };
    let bytes = percent_decode(path)?;
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

/// A `file://` URI for an absolute path, percent-encoding everything but
/// unreserved characters and `/`.
pub fn path_to_file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(exec: &str, targets: &[&str]) -> Vec<String> {
        let targets: Vec<String> = targets.iter().map(|s| s.to_string()).collect();
        let context = ExecContext {
            name: "Text Editor",
            icon: Some("accessories-text-editor"),
            location: Some(Path::new("/usr/share/applications/editor.desktop")),
        };
        Exec::parse(exec).expect("valid Exec").expand(&targets, &context)
    }

    fn invalid(exec: &str) -> bool {
        matches!(Exec::parse(exec), Err(Error::InvalidArgument(_)))
    }

    #[test]
    fn simple_lines_and_spaces() {
        assert_eq!(argv("foot", &[]), ["foot"]);
        assert_eq!(argv("  foot   --server  ", &[]), ["foot", "--server"]);
        assert_eq!(argv("/usr/bin/prog\t-a\n-b", &[]), ["/usr/bin/prog", "-a", "-b"]);
        assert_eq!(argv("env FOO=bar prog", &[]), ["env", "FOO=bar", "prog"]);
        assert_eq!(argv("prog -- --not-an-option", &[]), ["prog", "--", "--not-an-option"]);
        assert_eq!(argv("prog --opt=value -x", &[]), ["prog", "--opt=value", "-x"]);
    }

    #[test]
    fn the_specifications_quoting_examples() {
        // From the specification: `"\\$HOME"` in the file (after the
        // general unescape: `"\$HOME"`) is the literal argument `$HOME`.
        assert_eq!(argv(r#"echo "\$HOME""#, &[]), ["echo", "$HOME"]);
        // A literal backslash: `\\\\` in the file, `\\` here, `\` out.
        assert_eq!(argv(r#"prog "a\\b""#, &[]), ["prog", r"a\b"]);
        assert_eq!(argv(r#"prog "say \"hi\"" "`x`\`""#, &[]), ["prog", r#"say "hi""#, "`x``"]);
        assert_eq!(argv(r#"prog "with space" "" end"#, &[]), ["prog", "with space", "", "end"]);
        assert_eq!(argv(r#"prog "a b"c"d e""#, &[]), ["prog", "a bcd e"]);
        // Reserved characters are fine inside quotes.
        assert_eq!(argv(r#"sh -c "echo hi > /tmp/x; ls | wc -l""#, &[]), ["sh", "-c", "echo hi > /tmp/x; ls | wc -l"]);
        // Unknown escapes inside quotes stay as written.
        assert_eq!(argv(r#"prog "\n""#, &[]), ["prog", r"\n"]);
    }

    #[test]
    fn compatibility_quoting() {
        assert_eq!(argv(r"prog a\ b", &[]), ["prog", "a b"]);
        assert_eq!(argv("sh -c 'echo \"$1\"' sh", &[]), ["sh", "-c", "echo \"$1\"", "sh"]);
        assert_eq!(argv("prog ''", &[]), ["prog", ""]);
    }

    #[test]
    fn percent_signs() {
        assert_eq!(argv("prog 100%%", &[]), ["prog", "100%"]);
        assert_eq!(argv("prog %%f", &["/x"]), ["prog", "%f"]);
        assert_eq!(argv(r#"sh -c "date +%%Y""#, &[]), ["sh", "-c", "date +%Y"]);
        assert_eq!(argv("prog '%%'", &[]), ["prog", "%"]);
    }

    #[test]
    fn file_codes() {
        assert_eq!(argv("gimp %F", &["/a b.png", "/c.png"]), ["gimp", "/a b.png", "/c.png"]);
        assert_eq!(argv("gimp %f", &["/a.png", "/c.png"]), ["gimp", "/a.png"]);
        // No files: a standalone code disappears.
        assert_eq!(argv("gimp %f", &[]), ["gimp"]);
        assert_eq!(argv("gimp %F --last", &[]), ["gimp", "--last"]);
        // Inside a longer argument it expands to nothing.
        assert_eq!(argv("prog --file=%f", &[]), ["prog", "--file="]);
        assert_eq!(argv("prog --file=%f", &["/x y"]), ["prog", "--file=/x y"]);
        // file: URIs become paths; other URIs are skipped.
        assert_eq!(
            argv("prog %F", &["file:///home/me/a%20b.txt", "https://example.org/", "file://localhost/c"]),
            ["prog", "/home/me/a b.txt", "/c"]
        );
        assert_eq!(argv("prog %f", &["https://example.org/x", "file:///y"]), ["prog", "/y"]);
        assert_eq!(argv("prog %f", &["https://example.org/x"]), ["prog"]);
        assert_eq!(argv("prog %f", &["file://otherhost/x"]), ["prog"]);
        // A quoted %F can't become several arguments.
        assert_eq!(argv(r#"prog "%F""#, &["/a", "/b"]), ["prog", "/a"]);
        assert_eq!(argv(r#"prog "%f""#, &[]), ["prog", ""]);
    }

    #[test]
    fn url_codes() {
        assert_eq!(
            argv("firefox --name firefox %U", &["https://a.org/?q=1 2", "/tmp/x.html"]),
            ["firefox", "--name", "firefox", "https://a.org/?q=1 2", "/tmp/x.html"]
        );
        assert_eq!(argv("prog %u", &["mailto:me@example.org", "/b"]), ["prog", "mailto:me@example.org"]);
        assert_eq!(argv("prog %u", &[]), ["prog"]);
        assert_eq!(argv("prog --url=%u", &["file:///a"]), ["prog", "--url=file:///a"]);
    }

    #[test]
    fn other_codes() {
        assert_eq!(
            argv("prog %i %c %k", &[]),
            ["prog", "--icon", "accessories-text-editor", "Text Editor", "/usr/share/applications/editor.desktop"]
        );
        let bare = Exec::parse("prog %i %k %c").unwrap();
        assert_eq!(bare.expand(&[], &ExecContext::default()), ["prog", ""]);
        assert_eq!(argv("prog --title=%c", &[]), ["prog", "--title=Text Editor"]);
        // Deprecated codes are removed.
        assert_eq!(argv("prog %d %D %n %N %v %m -x%m", &[]), ["prog", "-x"]);
    }

    #[test]
    fn malformed_lines() {
        assert!(invalid(""));
        assert!(invalid("   "));
        assert!(invalid(r#"prog "unterminated"#));
        assert!(invalid("prog 'unterminated"));
        assert!(invalid(r#"prog "ends in backslash\"#));
        assert!(invalid(r"prog trailing\"));
        assert!(invalid("prog %x"));
        assert!(invalid("prog %"));
        assert!(invalid("%f"));
        assert!(invalid("FOO=bar prog"));
        assert!(invalid("%d"));
        assert!(invalid(r#""" prog"#));
        // A quoted program is fine.
        assert_eq!(argv(r#""/opt/My App/bin/app" %u"#, &[]), ["/opt/My App/bin/app"]);
    }

    #[test]
    fn instances_per_target() {
        let context = ExecContext::default();
        let targets: Vec<String> = ["/a", "/b", "https://x.org/"].iter().map(|s| s.to_string()).collect();
        let single = Exec::parse("viewer %f").unwrap();
        assert!(single.takes_targets() && !single.takes_many() && !single.takes_urls());
        assert_eq!(single.expand_each(&targets, &context), [vec!["viewer", "/a"], vec!["viewer", "/b"]]);
        let many = Exec::parse("viewer %F").unwrap();
        assert_eq!(many.expand_each(&targets, &context), [vec!["viewer", "/a", "/b"]]);
        let urls = Exec::parse("browser %u").unwrap();
        assert_eq!(urls.expand_each(&targets, &context).len(), 3);
        let none = Exec::parse("app").unwrap();
        assert!(!none.takes_targets());
        assert_eq!(none.expand_each(&targets, &context), [vec!["app"]]);
        assert_eq!(single.expand_each(&[], &context), [vec!["viewer"]]);
        assert_eq!(single.expand_each(&targets[2..], &context), [vec!["viewer"]]);
        assert_eq!(single.program(), "viewer");
    }

    #[test]
    fn relative_paths_become_absolute() {
        let out = argv("prog %f", &["notes.txt"]);
        assert!(Path::new(&out[1]).is_absolute());
        assert!(out[1].ends_with("/notes.txt"));
    }

    #[test]
    fn file_uris() {
        assert_eq!(file_uri_to_path("file:///a%2Fb/%C3%A9"), Some(PathBuf::from("/a/b/é")));
        assert_eq!(file_uri_to_path("FILE:/x"), Some(PathBuf::from("/x")));
        assert_eq!(file_uri_to_path("file:///x?q#f"), Some(PathBuf::from("/x")));
        assert_eq!(file_uri_to_path("file:///bad%2"), None);
        assert_eq!(file_uri_to_path("file:///bad%zz"), None);
        assert_eq!(file_uri_to_path("file://host"), None);
        assert_eq!(file_uri_to_path("file:relative"), None);
        assert_eq!(file_uri_to_path("http://x/"), None);
        assert_eq!(path_to_file_uri(Path::new("/a b/é#")), "file:///a%20b/%C3%A9%23");
        assert!(has_scheme("x-scheme+1.a:foo") && !has_scheme("1x:foo") && !has_scheme("nocolon"));
    }
}
