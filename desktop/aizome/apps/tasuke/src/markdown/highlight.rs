//! Code's colours, where they're cheap and say something: a shell
//! session's prompt and comments, configuration's comments and section
//! heads, and a diff's added and removed lines. Everything else is set
//! plain (no grammar is loaded for it).

use aizome_kit::Role;

use super::Run;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Shell,
    Config,
    Diff,
    Plain,
}

fn lang(name: &str) -> Lang {
    match name.to_ascii_lowercase().as_str() {
        "sh" | "bash" | "shell" | "console" | "zsh" | "fish" | "shell-session" => Lang::Shell,
        "toml" | "ini" | "conf" | "cfg" | "nix" | "yaml" | "yml" | "systemd" | "desktop" => Lang::Config,
        "diff" | "patch" => Lang::Diff,
        _ => Lang::Plain,
    }
}

/// `text`'s lines, as runs in their roles.
pub fn lines(name: &str, text: &str) -> Vec<Vec<Run>> {
    let lang = lang(name);
    text.split('\n').map(|line| line_runs(lang, line)).collect()
}

fn run(text: &str, role: Option<Role>) -> Run {
    Run { text: text.to_owned(), role, ..Run::default() }
}

fn line_runs(lang: Lang, line: &str) -> Vec<Run> {
    match lang {
        Lang::Plain => vec![run(line, None)],
        Lang::Diff => {
            let role = match crate::diff::kind(line) {
                crate::diff::Line::Added => Some(Role::Success),
                crate::diff::Line::Removed => Some(Role::Danger),
                crate::diff::Line::Note => Some(Role::Cyan),
                crate::diff::Line::Same => None,
            };
            vec![run(line, role)]
        }
        Lang::Shell | Lang::Config => {
            let trimmed = line.trim_start();
            if lang == Lang::Config && trimmed.starts_with('[') && trimmed.trim_end().ends_with(']') {
                return vec![run(line, Some(Role::Violet))];
            }
            let mut out = Vec::new();
            let mut rest = line;
            // A prompt, as a session shows it: `$ ` and `# ` (root's) only
            // where the line starts with one.
            if lang == Lang::Shell
                && let Some(after) = rest.strip_prefix("$ ")
            {
                out.push(run("$ ", Some(Role::Faint)));
                rest = after;
            }
            match comment_start(rest) {
                Some(at) => {
                    if at > 0 {
                        out.push(run(&rest[..at], None));
                    }
                    out.push(run(&rest[at..], Some(Role::Text3)));
                }
                None => out.push(run(rest, None)),
            }
            out
        }
    }
}

/// Where a `#` comment starts: at the line's start or after whitespace,
/// outside quotes.
fn comment_start(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '#') if prev.is_whitespace() => return Some(i),
            _ => {}
        }
        prev = c;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_comments_and_prompts_but_not_hashes_in_words_or_quotes() {
        let l = lines("sh", "$ wpctl status # the devices\necho 'a # b' x#y");
        assert_eq!(l[0][0].role, Some(Role::Faint));
        assert_eq!(l[0].last().unwrap().text, "# the devices");
        assert_eq!(l[0].last().unwrap().role, Some(Role::Text3));
        assert_eq!(l[1].len(), 1, "{:?}", l[1]);
    }

    #[test]
    fn diffs_and_sections() {
        let l = lines("diff", "@@ -1 +1 @@\n-old\n+new");
        assert_eq!(l.iter().map(|r| r[0].role).collect::<Vec<_>>(), [Some(Role::Cyan), Some(Role::Danger), Some(Role::Success)]);
        assert_eq!(lines("toml", "[model]")[0][0].role, Some(Role::Violet));
        assert_eq!(lines("rust", "fn x() {} # no")[0].len(), 1);
    }
}
