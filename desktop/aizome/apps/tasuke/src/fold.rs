//! How a transcript reads at a glance: a run of looking (read-only
//! commands, files and notes read, windows listed) folds into one line,
//! "Looked at 6 things", that opens to show each step. What changes
//! something, asks, fails to be understood or shows a picture stands on
//! its own, and so does what the model says between steps.
//!
//! The model's thinking between two looks belongs to the run: it is part
//! of the looking, and opens with it. Thinking after the last look stays
//! out, beside the answer it led to.

use crate::transcript::{Body, Item, Tool, ToolState};

/// One line of the transcript as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// An item on its own, by its index in the transcript.
    Item(usize),
    /// A run of looking.
    Looked(Run),
}

/// Consecutive looks (and the thinking between them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// Its first item's id: what its open or closed state is kept by, the
    /// same while the run grows.
    pub key: u64,
    /// Its items' indices in the transcript, in order.
    pub items: Vec<usize>,
}

/// What a run's line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Headline {
    /// The looks in it.
    pub count: usize,
    /// One is still waiting or running.
    pub open: bool,
    /// How many went wrong (a non-zero exit, a timeout).
    pub failed: usize,
    /// Its targets, briefly, latest last.
    pub targets: Vec<String>,
}

impl Headline {
    /// "Looked at 6 things", "Looking at 3 things…", "Looked at 1 thing".
    pub fn words(&self) -> String {
        let things = if self.count == 1 { "thing" } else { "things" };
        if self.open {
            format!("Looking at {} {things}…", self.count)
        } else {
            format!("Looked at {} {things}", self.count)
        }
    }

    /// The targets, as the line shows them after the words.
    pub fn summary(&self) -> String {
        self.targets.join(" · ")
    }
}

fn tool(items: &[Item], i: usize) -> Option<&Tool> {
    match &items.get(i)?.body {
        Body::Tool(t) => Some(&**t),
        _ => None,
    }
}

fn is_look(items: &[Item], i: usize) -> bool {
    tool(items, i).is_some_and(|t| t.looks)
}

fn is_thinking(items: &[Item], i: usize) -> bool {
    matches!(items.get(i).map(|i| &i.body), Some(Body::Reasoning(_)))
}

/// The transcript's rows: runs of two looks or more folded, the rest as
/// they are.
pub fn rows(items: &[Item]) -> Vec<Row> {
    let mut out = Vec::with_capacity(items.len());
    let mut i = 0;
    while i < items.len() {
        if !is_look(items, i) {
            out.push(Row::Item(i));
            i += 1;
            continue;
        }
        // Take looks and thinking, then give back the thinking after the
        // last look.
        let mut end = i;
        while end < items.len() && (is_look(items, end) || is_thinking(items, end)) {
            end += 1;
        }
        while end > i && !is_look(items, end - 1) {
            end -= 1;
        }
        let run: Vec<usize> = (i..end).collect();
        let looks = run.iter().filter(|&&k| is_look(items, k)).count();
        if looks >= 2 {
            out.push(Row::Looked(Run { key: items[i].id, items: run }));
        } else {
            out.extend(run.into_iter().map(Row::Item));
        }
        i = end;
    }
    out
}

/// A target cut to what reads at a glance: a command's first line, a
/// file's name.
fn brief(t: &Tool) -> String {
    let target = t.described.target.lines().next().unwrap_or("").trim();
    let target = if t.call.name == "read_file" { target.rsplit('/').next().unwrap_or(target) } else { target };
    const MOST: usize = 40;
    if target.chars().count() > MOST {
        let cut: String = target.chars().take(MOST - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        target.to_owned()
    }
}

impl Run {
    pub fn headline(&self, items: &[Item]) -> Headline {
        let tools: Vec<&Tool> = self.items.iter().filter_map(|&i| tool(items, i)).collect();
        let mut targets: Vec<String> = Vec::new();
        for t in &tools {
            let b = brief(t);
            if !b.is_empty() && !targets.contains(&b) {
                targets.push(b);
            }
        }
        Headline {
            count: tools.len(),
            open: tools.iter().any(|t| t.state.is_open()),
            // A look that ran and exited non-zero answered (grep found
            // nothing, coredumpctl has no crashes); one that couldn't run or
            // took too long didn't.
            failed: tools
                .iter()
                .filter(|t| matches!(t.state, ToolState::TimedOut | ToolState::Failed { exit: None }))
                .count(),
            targets,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::{Approval, Event, ToolCall, ToolOutput};
    use crate::transcript::Transcript;

    fn look(t: &mut Transcript, id: &str, command: &str, exit: Option<i32>) {
        t.apply(
            Event::ToolCall {
                id: id.into(),
                call: ToolCall::new("run_command", json!({ "command": command })),
                needs: Approval::None,
            },
            0,
        );
        if let Some(code) = exit {
            t.apply(
                Event::ToolOutput { id: id.into(), output: ToolOutput { exit: Some(code), ..Default::default() } },
                1,
            );
        }
    }

    fn shape(t: &Transcript) -> Vec<String> {
        rows(&t.items)
            .iter()
            .map(|r| match r {
                Row::Item(i) => format!("item {i}"),
                Row::Looked(run) => format!("looked {:?}", run.items),
            })
            .collect()
    }

    #[test]
    fn consecutive_looks_fold_and_a_lone_one_stands() {
        let mut t = Transcript::new();
        t.user("Why is there no sound?", 0);
        look(&mut t, "a", "wpctl status", Some(0));
        t.apply(Event::ReasoningDelta { text: "The sink is muted?".into() }, 2);
        look(&mut t, "b", "pactl list sinks", Some(0));
        look(&mut t, "c", "journalctl --user -b -u wireplumber --no-pager", Some(1));
        t.apply(Event::ReasoningDelta { text: "Muted.".into() }, 3);
        t.apply(Event::TextDelta { text: "It's muted.".into() }, 4);
        look(&mut t, "d", "wpctl get-volume @DEFAULT_AUDIO_SINK@", Some(0));
        // user, [a, thinking, b, c], thinking after, answer, d alone
        assert_eq!(shape(&t), ["item 0", "looked [1, 2, 3, 4]", "item 5", "item 6", "item 7"]);
        let Row::Looked(run) = &rows(&t.items)[1] else { panic!() };
        assert_eq!(run.key, t.items[1].id, "kept by its first item");
        let h = run.headline(&t.items);
        assert_eq!((h.count, h.open, h.failed), (3, false, 0), "an exit of 1 is an answer");
        assert_eq!(h.words(), "Looked at 3 things");
        assert_eq!(h.targets[0], "wpctl status");
        assert!(h.targets[2].ends_with('…') && h.targets[2].chars().count() == 40, "{}", h.targets[2]);
    }

    #[test]
    fn what_asks_or_changes_breaks_a_run() {
        let mut t = Transcript::new();
        look(&mut t, "a", "df -h", Some(0));
        t.apply(
            Event::ToolCall {
                id: "b".into(),
                call: ToolCall::new("run_command", json!({"command": "rm -rf ~/.cache/thumbnails"})),
                needs: Approval::Ask { reason: "It deletes files".into() },
            },
            1,
        );
        look(&mut t, "c", "du -sh ~/.cache", Some(0));
        t.apply(
            Event::ToolCall { id: "s".into(), call: ToolCall::new("screenshot", json!({})), needs: Approval::None },
            2,
        );
        look(&mut t, "d", "ls", Some(0));
        assert_eq!(shape(&t), ["item 0", "item 1", "item 2", "item 3", "item 4"]);
    }

    #[test]
    fn a_growing_run_says_so_and_keeps_its_key() {
        let mut t = Transcript::new();
        look(&mut t, "a", "df -h", Some(0));
        look(&mut t, "b", "du -xh --max-depth=1 ~", None);
        let Row::Looked(first) = rows(&t.items)[0].clone() else { panic!() };
        let h = first.headline(&t.items);
        assert!(h.open);
        assert_eq!(h.words(), "Looking at 2 things…");
        look(&mut t, "c", "du -xh --max-depth=1 ~/.cache", None);
        let Row::Looked(grown) = rows(&t.items)[0].clone() else { panic!() };
        assert_eq!(grown.key, first.key);
        assert_eq!(grown.items.len(), 3);
    }
}
