//! The matched program in Aizome's kit (docs/performance/why-toolkits.md),
//! the same as the GTK 4, libadwaita, Qt Widgets and Qt Quick programs beside
//! it:
//!
//!   tk-kit text FILE        the kit's text editor with FILE, the caret at
//!                           the start of line 20, no wrapping
//!   tk-kit list [ROWS]      a virtual list of ROWS (10,000) rows, 24 pt each
//!
//! Options: `--scroll ROWS MS` (every MS ms scroll down ROWS rows, 0 = a
//! page, from a timer, wrapping at the end), `--size W H` (1280 x 800).
//!
//! Fonts as in the other programs: IBM Plex Mono at 14 pt in the editor
//! (its natural line height, 1.3), Archivo at 14 pt in the list. No
//! line numbers or current-line band (the other toolkits' text views have
//! neither); otherwise the kit's defaults.
use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{EditorAction, EditorContent, Selection};

#[derive(Clone, Debug)]
enum Msg {
    Edit(EditorAction),
    Start,
    Step,
}

struct Tk {
    text: bool,
    content: EditorContent,
    rows: usize,
    rows_text: Arc<Vec<String>>,
    reveal: usize,
    scroll_rows: i64,
    scroll_ms: u64,
    size: (f64, f64),
}

/// Rows the list shows at once: 800 pt of window.
fn page(size: (f64, f64)) -> usize {
    (size.1 / 24.0).floor() as usize
}

impl Program for Tk {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.tk.Kit", "tk", "tk")
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("tk").size(self.size.0, self.size.1));
        if self.text {
            cx.focus("editor");
        }
        if self.scroll_rows >= 0 && self.scroll_ms > 0 {
            cx.after(Duration::from_millis(1500), Msg::Start);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Edit(a) => {
                self.content.perform(a);
            }
            Msg::Start => {
                cx.every(Duration::from_millis(self.scroll_ms), Msg::Step);
            }
            Msg::Step => {
                let by = if self.scroll_rows > 0 { self.scroll_rows as usize } else { page(self.size) };
                self.reveal += by;
                if self.reveal >= self.rows {
                    self.reveal = page(self.size) - 1;
                }
            }
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        if self.text {
            text_editor(&self.content)
                .mono(true)
                .size(14.0)
                .line_height(1.3)
                .wrap(false)
                .current_line(false)
                .on_action(Msg::Edit)
                .id("editor")
                .into()
        } else {
            let rows = self.rows_text.clone();
            virtual_list(self.rows, 24.0, move |i| text(rows[i].clone()).size(14.0)).reveal(self.reveal).into()
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("text");
    let mut i = 2;
    let mut tk = Tk {
        text: mode == "text",
        content: EditorContent::new(""),
        rows: 10_000,
        rows_text: Arc::new(Vec::new()),
        reveal: 0,
        scroll_rows: -1,
        scroll_ms: 0,
        size: (1280.0, 800.0),
    };
    if tk.text {
        let file = args.get(i).expect("text FILE");
        i += 1;
        let text = std::fs::read_to_string(file).expect("can't read the file");
        tk.content = EditorContent::new(&text);
        let at = tk.content.rope().line_to_char(19);
        tk.content.set_selection(Selection::caret(at));
    } else if let Some(n) = args.get(i).filter(|a| !a.starts_with('-')) {
        tk.rows = n.parse().expect("ROWS");
        i += 1;
    }
    while i < args.len() {
        match args[i].as_str() {
            "--scroll" => {
                tk.scroll_rows = args[i + 1].parse().expect("ROWS");
                tk.scroll_ms = args[i + 2].parse().expect("MS");
                i += 3;
            }
            "--size" => {
                tk.size = (args[i + 1].parse().expect("W"), args[i + 2].parse().expect("H"));
                i += 3;
            }
            a => panic!("unknown argument {a}"),
        }
    }
    let n = tk.rows;
    tk.rows_text = Arc::new(
        (0..n).map(|r| format!("Row {:05}  fixture item {} of {n}, in a list of generated rows", r + 1, r + 1)).collect(),
    );
    tk.reveal = page(tk.size) - 1;
    aizome_kit::run(tk)
}
