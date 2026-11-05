//! Markdown, as answers are written: parsed into blocks of styled runs
//! that hold roles, not colours, so a document is drawn in whichever theme
//! is current ([`view`]).
//!
//! Adapted from Omperor's `src/markdown/mod.rs` (MIT OR Apache-2.0, by the
//! same author), without its math, diagrams and pictures: paragraphs,
//! headings, emphasis, strikethrough, inline and fenced code, links,
//! quotes and GitHub's callouts, ordered, bulleted and task lists,
//! tables and rules. Code is coloured only where that's cheap and helps
//! most here: comments in shell and configuration, and diffs' lines
//! ([`highlight`]).

pub mod highlight;
pub mod view;

use std::sync::Arc;

use aizome_kit::Role;
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A parsed answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(Vec<Run>),
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    /// Fenced or indented code: its language as written, its text, and
    /// its lines coloured.
    Code {
        lang: String,
        text: Arc<str>,
        lines: Arc<Vec<Vec<Run>>>,
    },
    Quote(Vec<Block>),
    Callout {
        kind: Callout,
        blocks: Vec<Block>,
    },
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Table {
        aligns: Vec<Align>,
        head: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
    },
    Rule,
}

/// One list item: a task's box, when it has one, and what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Callout {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl Callout {
    pub fn title(self) -> &'static str {
        match self {
            Callout::Note => "Note",
            Callout::Tip => "Tip",
            Callout::Important => "Important",
            Callout::Warning => "Warning",
            Callout::Caution => "Caution",
        }
    }

    pub fn role(self) -> Role {
        match self {
            Callout::Note => Role::Cyan,
            Callout::Tip => Role::Success,
            Callout::Important => Role::Violet,
            Callout::Warning => Role::Warning,
            Callout::Caution => Role::Danger,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Start,
    Center,
    End,
}

/// A run of text in one style.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<Arc<str>>,
    /// A colour of its own (code's tokens).
    pub role: Option<Role>,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Run {
        Run { text: text.into(), ..Run::default() }
    }

    fn same_style(&self, other: &Run) -> bool {
        (self.bold, self.italic, self.strike, self.code, &self.link, self.role)
            == (other.bold, other.italic, other.strike, other.code, &other.link, other.role)
    }
}

/// The style the open inline tags give text.
#[derive(Debug, Clone, Default)]
struct Style {
    bold: u32,
    italic: u32,
    strike: u32,
    /// Inside `<kbd>`, `<code>` or `<tt>`: set as code.
    code: u32,
    link: Vec<Arc<str>>,
}

impl Style {
    fn run(&self, text: &str) -> Run {
        Run {
            text: text.to_owned(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            link: self.link.last().cloned(),
            code: self.code > 0,
            role: None,
        }
    }
}

/// Where blocks are being gathered: the document, a quote, a list item.
#[derive(Debug)]
enum Frame {
    Root(Vec<Block>),
    Quote(Option<Callout>, Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Option<bool>, Vec<Block>),
}

impl Frame {
    fn blocks(&mut self) -> Option<&mut Vec<Block>> {
        match self {
            Frame::Root(b) | Frame::Quote(_, b) | Frame::Item(_, b) => Some(b),
            Frame::List(..) => None,
        }
    }
}

/// A table being read: its columns' alignment, its head, its rows, the
/// row being read.
type TableDraft = (Vec<Align>, Vec<Vec<Run>>, Vec<Vec<Vec<Run>>>, Vec<Vec<Run>>);

struct Builder {
    frames: Vec<Frame>,
    /// Runs of the paragraph, heading or cell being written.
    runs: Option<Vec<Run>>,
    style: Style,
    heading: Option<u8>,
    code: Option<(String, String)>,
    /// An image's alt text, read until it ends: shown as a link to it.
    image: Option<(String, String)>,
    table: Option<TableDraft>,
}

impl Builder {
    fn push_block(&mut self, block: Block) {
        // Text directly in a list item (a tight list) belongs to it; so does
        // anything else: find the nearest frame that holds blocks.
        for frame in self.frames.iter_mut().rev() {
            if let Some(blocks) = frame.blocks() {
                blocks.push(block);
                return;
            }
        }
    }

    fn push_run(&mut self, run: Run) {
        if run.text.is_empty() {
            return;
        }
        let runs = self.runs.get_or_insert_with(Vec::new);
        match runs.last_mut() {
            Some(last) if last.same_style(&run) => last.text.push_str(&run.text),
            _ => runs.push(run),
        }
    }

    /// The runs written so far become a paragraph (text in a tight list
    /// item has no paragraph tags of its own).
    fn flush_runs(&mut self) {
        if let Some(runs) = self.runs.take() {
            let runs = trim_runs(runs);
            if !runs.is_empty() {
                self.push_block(Block::Paragraph(runs));
            }
        }
    }
}

fn trim_runs(mut runs: Vec<Run>) -> Vec<Run> {
    if let Some(first) = runs.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = runs.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    runs.retain(|r| !r.text.is_empty());
    runs
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn callout(kind: Option<BlockQuoteKind>) -> Option<Callout> {
    kind.map(|k| match k {
        BlockQuoteKind::Note => Callout::Note,
        BlockQuoteKind::Tip => Callout::Tip,
        BlockQuoteKind::Important => Callout::Important,
        BlockQuoteKind::Warning => Callout::Warning,
        BlockQuoteKind::Caution => Callout::Caution,
    })
}

/// Parse `text` into a document. Cheap enough for the interface's thread
/// at an answer's length, so an answer is parsed again as it streams.
pub fn parse(text: &str) -> Doc {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_FOOTNOTES;
    let mut b = Builder {
        frames: vec![Frame::Root(Vec::new())],
        runs: None,
        style: Style::default(),
        heading: None,
        code: None,
        image: None,
        table: None,
    };
    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => b.flush_runs(),
                Tag::Heading { level, .. } => {
                    b.flush_runs();
                    b.heading = Some(heading_level(level));
                    b.runs = Some(Vec::new());
                }
                Tag::BlockQuote(kind) => {
                    b.flush_runs();
                    b.frames.push(Frame::Quote(callout(kind), Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    b.flush_runs();
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_owned(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    b.code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    b.flush_runs();
                    b.frames.push(Frame::List(start, Vec::new()));
                }
                Tag::Item => {
                    b.flush_runs();
                    b.frames.push(Frame::Item(None, Vec::new()));
                }
                Tag::Table(aligns) => {
                    b.flush_runs();
                    let aligns = aligns
                        .iter()
                        .map(|a| match a {
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::End,
                            _ => Align::Start,
                        })
                        .collect();
                    b.table = Some((aligns, Vec::new(), Vec::new(), Vec::new()));
                }
                Tag::TableHead | Tag::TableRow => {
                    if let Some(t) = &mut b.table {
                        t.3.clear();
                    }
                }
                Tag::TableCell => b.runs = Some(Vec::new()),
                Tag::Emphasis => b.style.italic += 1,
                Tag::Strong => b.style.bold += 1,
                Tag::Strikethrough => b.style.strike += 1,
                Tag::Link { dest_url, .. } => b.style.link.push(Arc::from(dest_url.as_ref())),
                Tag::Image { dest_url, .. } => b.image = Some((dest_url.to_string(), String::new())),
                Tag::FootnoteDefinition(label) => {
                    b.flush_runs();
                    b.push_run(Run { text: format!("[{label}] "), bold: true, ..Run::default() });
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph => b.flush_runs(),
                TagEnd::Heading(_) => {
                    let runs = trim_runs(b.runs.take().unwrap_or_default());
                    let level = b.heading.take().unwrap_or(1);
                    if !runs.is_empty() {
                        b.push_block(Block::Heading { level, runs });
                    }
                }
                TagEnd::BlockQuote(_) => {
                    b.flush_runs();
                    if let Some(Frame::Quote(kind, blocks)) = b.frames.pop() {
                        b.push_block(match kind {
                            Some(kind) => Block::Callout { kind, blocks },
                            None => Block::Quote(blocks),
                        });
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, mut text)) = b.code.take() {
                        if text.ends_with('\n') {
                            text.pop();
                        }
                        let lines = highlight::lines(&lang, &text);
                        b.push_block(Block::Code { lang, text: Arc::from(text), lines: Arc::new(lines) });
                    }
                }
                TagEnd::List(_) => {
                    b.flush_runs();
                    if let Some(Frame::List(start, items)) = b.frames.pop() {
                        b.push_block(Block::List { start, items });
                    }
                }
                TagEnd::Item => {
                    b.flush_runs();
                    if let Some(Frame::Item(task, blocks)) = b.frames.pop()
                        && let Some(Frame::List(_, items)) = b.frames.last_mut()
                    {
                        items.push(Item { task, blocks });
                    }
                }
                TagEnd::TableCell => {
                    let runs = trim_runs(b.runs.take().unwrap_or_default());
                    if let Some(t) = &mut b.table {
                        t.3.push(runs);
                    }
                }
                TagEnd::TableHead => {
                    if let Some(t) = &mut b.table {
                        t.1 = std::mem::take(&mut t.3);
                    }
                }
                TagEnd::TableRow => {
                    if let Some(t) = &mut b.table {
                        let row = std::mem::take(&mut t.3);
                        t.2.push(row);
                    }
                }
                TagEnd::Table => {
                    if let Some((aligns, head, rows, _)) = b.table.take() {
                        b.push_block(Block::Table { aligns, head, rows });
                    }
                }
                TagEnd::Emphasis => b.style.italic = b.style.italic.saturating_sub(1),
                TagEnd::Strong => b.style.bold = b.style.bold.saturating_sub(1),
                TagEnd::Strikethrough => b.style.strike = b.style.strike.saturating_sub(1),
                TagEnd::Link => {
                    b.style.link.pop();
                }
                TagEnd::Image => {
                    // Pictures aren't shown in answers: a link to it.
                    if let Some((url, alt)) = b.image.take() {
                        let label = if alt.is_empty() { url.clone() } else { alt };
                        b.push_run(Run { text: label, link: Some(Arc::from(url.as_str())), ..Run::default() });
                    }
                }
                TagEnd::FootnoteDefinition => b.flush_runs(),
                _ => {}
            },
            Event::Text(text) => {
                if let Some((_, code)) = &mut b.code {
                    code.push_str(&text);
                } else if let Some((_, alt)) = &mut b.image {
                    alt.push_str(&text);
                } else {
                    let run = b.style.run(&text);
                    b.push_run(run);
                }
            }
            Event::Code(text) => {
                let mut run = b.style.run(&text);
                run.code = true;
                b.push_run(run);
            }
            Event::InlineMath(source) | Event::DisplayMath(source) => {
                let mut run = b.style.run(&source);
                run.code = true;
                b.push_run(run);
            }
            Event::InlineHtml(html) => match inline_tag(&html) {
                // The few tags answers use for type: kept as their look.
                Some(("kbd" | "code" | "tt", open)) => {
                    b.style.code = if open { b.style.code + 1 } else { b.style.code.saturating_sub(1) };
                }
                Some(("br", _)) => {
                    let run = b.style.run("\n");
                    b.push_run(run);
                }
                Some(("b" | "strong", open)) => {
                    b.style.bold = if open { b.style.bold + 1 } else { b.style.bold.saturating_sub(1) }
                }
                Some(("i" | "em", open)) => {
                    b.style.italic = if open { b.style.italic + 1 } else { b.style.italic.saturating_sub(1) }
                }
                Some(_) => {}
                // Anything else is shown as written: markup is text to read.
                None => {
                    let run = b.style.run(&html);
                    b.push_run(run);
                }
            },
            Event::Html(html) => {
                let run = b.style.run(&html);
                b.push_run(run);
            }
            Event::SoftBreak => {
                let run = b.style.run(" ");
                b.push_run(run);
            }
            Event::HardBreak => {
                let run = b.style.run("\n");
                b.push_run(run);
            }
            Event::Rule => {
                b.flush_runs();
                b.push_block(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some(Frame::Item(task, _)) = b.frames.last_mut() {
                    *task = Some(done);
                }
            }
            Event::FootnoteReference(label) => {
                let mut run = b.style.run(&format!("[{label}]"));
                run.role = Some(Role::Accent);
                b.push_run(run);
            }
        }
    }
    b.flush_runs();
    // Unclosed frames (an answer still streaming) give back what they hold.
    if let Some((lang, text)) = b.code.take() {
        let lines = highlight::lines(&lang, &text);
        b.push_block(Block::Code { lang, text: Arc::from(text), lines: Arc::new(lines) });
    }
    while b.frames.len() > 1 {
        match b.frames.pop() {
            Some(Frame::Quote(kind, blocks)) => b.push_block(match kind {
                Some(kind) => Block::Callout { kind, blocks },
                None => Block::Quote(blocks),
            }),
            Some(Frame::List(start, items)) => b.push_block(Block::List { start, items }),
            Some(Frame::Item(task, blocks)) => {
                if let Some(Frame::List(_, items)) = b.frames.last_mut() {
                    items.push(Item { task, blocks });
                } else {
                    for block in blocks {
                        b.push_block(block);
                    }
                }
            }
            _ => {}
        }
    }
    match b.frames.pop() {
        Some(Frame::Root(blocks)) => Doc { blocks },
        _ => Doc::default(),
    }
}

/// A simple inline tag: its name, lowercased, and whether it opens.
fn inline_tag(html: &str) -> Option<(&'static str, bool)> {
    let inner = html.trim().strip_prefix('<')?.strip_suffix('>')?.trim_end_matches('/').trim();
    let (open, name) = match inner.strip_prefix('/') {
        Some(name) => (false, name),
        None => (true, inner),
    };
    let name = name.split_whitespace().next()?.to_ascii_lowercase();
    ["kbd", "code", "tt", "br", "b", "strong", "i", "em", "sup", "sub", "span", "small", "mark", "u"]
        .into_iter()
        .find(|tag| *tag == name)
        .map(|tag| (tag, open))
}

/// The text of `runs`, as copied.
pub fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Doc) -> Vec<String> {
        doc.blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(r) | Block::Heading { runs: r, .. } => runs_text(r),
                Block::Code { text, .. } => text.to_string(),
                other => format!("{other:?}").chars().take(12).collect(),
            })
            .collect()
    }

    #[test]
    fn paragraphs_headings_and_emphasis() {
        let doc = parse("# Title\n\nSome **bold** and *italic* and `code` and [a link](https://x.test).\n\nNext.");
        assert_eq!(texts(&doc), ["Title", "Some bold and italic and code and a link.", "Next."]);
        let Block::Paragraph(runs) = &doc.blocks[1] else { panic!() };
        assert!(runs.iter().any(|r| r.bold && r.text == "bold"));
        assert!(runs.iter().any(|r| r.italic && r.text == "italic"));
        assert!(runs.iter().any(|r| r.code && r.text == "code"));
        assert!(runs.iter().any(|r| r.link.as_deref() == Some("https://x.test") && r.text == "a link"));
    }

    #[test]
    fn lists_nest_and_tasks_keep_their_boxes() {
        let doc = parse("- one\n- two\n  - deeper\n\n1. first\n2. second\n\n- [x] done\n- [ ] open");
        let Block::List { start: None, items } = &doc.blocks[0] else { panic!("{doc:?}") };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[1].blocks.last(), Some(Block::List { .. })));
        assert!(matches!(&doc.blocks[1], Block::List { start: Some(1), items } if items.len() == 2));
        let Block::List { items, .. } = &doc.blocks[2] else { panic!() };
        assert_eq!(items.iter().map(|i| i.task).collect::<Vec<_>>(), [Some(true), Some(false)]);
    }

    #[test]
    fn tables_quotes_and_callouts() {
        let doc = parse("| A | B |\n|:--|--:|\n| 1 | 2 |\n\n> quoted\n\n> [!WARNING]\n> careful\n\n---");
        let Block::Table { aligns, head, rows } = &doc.blocks[0] else { panic!("{doc:?}") };
        assert_eq!(aligns, &[Align::Start, Align::End]);
        assert_eq!(runs_text(&head[1]), "B");
        assert_eq!(runs_text(&rows[0][0]), "1");
        assert!(matches!(&doc.blocks[1], Block::Quote(b) if b.len() == 1));
        assert!(matches!(&doc.blocks[2], Block::Callout { kind: Callout::Warning, .. }));
        assert_eq!(doc.blocks[3], Block::Rule);
    }

    #[test]
    fn an_answer_cut_off_mid_stream_still_shows() {
        let doc = parse("Intro\n\n- one\n- tw");
        assert_eq!(doc.blocks.len(), 2);
        let doc = parse("```sh\nsystemctl --user status");
        assert!(
            matches!(&doc.blocks[0], Block::Code { text, lang, .. } if &**text == "systemctl --user status" && lang == "sh")
        );
    }

    #[test]
    fn markup_that_isnt_type_reads_as_written() {
        let doc = parse("Press <kbd>❖</kbd> and <custom>x</custom>.");
        let Block::Paragraph(runs) = &doc.blocks[0] else { panic!() };
        assert!(runs.iter().any(|r| r.code && r.text == "❖"));
        assert!(runs_text(runs).contains("<custom>"));
    }
}
