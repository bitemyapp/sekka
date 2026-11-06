//! A file change as the approval card shows it: a unified diff of lines,
//! with three lines of context, from the longest common subsequence.
//! Files the agent edits are configuration, short enough for that; a
//! change too large to compare says so rather than taking the UI thread.

/// Lines past which two texts aren't compared.
const MOST: usize = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Same,
    Removed,
    Added,
    /// `@@` and other notes.
    Note,
}

/// The kind of a unified diff's line, by its first character.
pub fn kind(line: &str) -> Line {
    match line.as_bytes().first() {
        Some(b'+') if !line.starts_with("+++") => Line::Added,
        Some(b'-') if !line.starts_with("---") => Line::Removed,
        Some(b'@') => Line::Note,
        _ => Line::Same,
    }
}

/// `old` to `new`, as `-`, `+` and ` ` lines in hunks under `@@` heads.
pub fn unified(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    if a.len() > MOST || b.len() > MOST {
        return format!("@@ {} lines replaced by {} @@", a.len(), b.len());
    }
    // The table of common subsequence lengths, from the ends.
    let (n, m) = (a.len(), b.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * (m + 1) + j] = if a[i] == b[j] {
                t[(i + 1) * (m + 1) + j + 1] + 1
            } else {
                t[(i + 1) * (m + 1) + j].max(t[i * (m + 1) + j + 1])
            };
        }
    }
    let mut ops: Vec<(Line, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((Line::Same, a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || t[(i + 1) * (m + 1) + j] >= t[i * (m + 1) + j + 1]) {
            // What went comes before what replaced it.
            ops.push((Line::Removed, a[i]));
            i += 1;
        } else {
            ops.push((Line::Added, b[j]));
            j += 1;
        }
    }
    // Hunks: changes with three lines of context around them.
    const CONTEXT: usize = 3;
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, (k, _))| *k != Line::Same).map(|(i, _)| i).collect();
    if changed.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(CONTEXT);
        let mut end = (changed[k] + CONTEXT + 1).min(ops.len());
        while k + 1 < changed.len() && changed[k + 1] <= end + CONTEXT {
            k += 1;
            end = (changed[k] + CONTEXT + 1).min(ops.len());
        }
        // Where the hunk starts in each file (1-based) and how long it is.
        let (mut old_at, mut new_at) = (1, 1);
        for (kind, _) in &ops[..start] {
            match kind {
                Line::Same => {
                    old_at += 1;
                    new_at += 1;
                }
                Line::Removed => old_at += 1,
                Line::Added => new_at += 1,
                Line::Note => {}
            }
        }
        let olds = ops[start..end].iter().filter(|(k, _)| *k != Line::Added).count();
        let news = ops[start..end].iter().filter(|(k, _)| *k != Line::Removed).count();
        out.push_str(&format!("@@ -{old_at},{olds} +{new_at},{news} @@\n"));
        for (kind, line) in &ops[start..end] {
            out.push(match kind {
                Line::Same | Line::Note => ' ',
                Line::Removed => '-',
                Line::Added => '+',
            });
            out.push_str(line);
            out.push('\n');
        }
        k += 1;
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_line_with_its_context() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nb\nc\nd\nE\nf\ng\nh\n";
        let d = unified(old, new);
        assert_eq!(d, "@@ -2,7 +2,7 @@\n b\n c\n d\n-e\n+E\n f\n g\n h");
        assert_eq!(kind("-e"), Line::Removed);
        assert_eq!(kind("+E"), Line::Added);
        assert_eq!(kind("@@ -2,7 +2,7 @@"), Line::Note);
        assert_eq!(kind(" b"), Line::Same);
    }

    #[test]
    fn a_new_file_is_all_added_and_nothing_changed_is_empty() {
        assert_eq!(unified("", "x\ny"), "@@ -1,0 +1,2 @@\n+x\n+y");
        assert_eq!(unified("same\n", "same\n"), "");
    }

    #[test]
    fn far_apart_changes_are_hunks_of_their_own() {
        let line = |i: usize| match i {
            2 => "two\n".to_owned(),
            27 => "twenty-seven\n".to_owned(),
            i => format!("{i}\n"),
        };
        let old: String = (0..30).map(|i| format!("{i}\n")).collect();
        let new: String = (0..30).map(line).collect();
        let d = unified(&old, &new);
        assert_eq!(d.matches("@@ -").count(), 2, "{d}");
    }
}
