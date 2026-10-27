//! Soroban, the program: its state, its messages, its window and menus.

use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key as KeyName, KeyEvent, NamedKey};
use aizome_kit::{column, row};
use aizome_search::calc::Angle;
use serde::{Deserialize, Serialize};

use crate::bits;
use crate::display::{self, Indicator};
use crate::entry::{Calc, Key, Mode, Num, TapeEntry};
use crate::keypad::{self, Face, GAP, GROUP_GAP};
use crate::tape;

pub const APP_ID: &str = "org.aizome.Soroban";

/// Space around the panel's parts.
pub const PAD: f64 = 12.0;
/// The toolbar's height at the medium size (the theme's `toolbar`).
pub const TOOLBAR: f64 = 30.0;
/// Programmer mode's radix rows and bit toggles.
pub const PROGRAMMER_EXTRA: f64 = 4.0 * bits::ROW + PAD + bits::HEIGHT;

/// Word sizes, as the segmented control offers them.
pub const WORDS: [u32; 4] = [8, 16, 32, 64];
const RADIXES: [(u32, &str); 4] = [(16, "HEX"), (10, "DEC"), (8, "OCT"), (2, "BIN")];

/// The window's (fixed) size for a mode, with the tape or without, at the
/// medium size.
pub fn window_size(mode: Mode, tape: bool) -> Size {
    window_size_in(TOOLBAR, mode, tape)
}

/// The window's size under a toolbar `toolbar` tall.
pub fn window_size_in(toolbar: f64, mode: Mode, tape: bool) -> Size {
    let pads = keypad::size(mode);
    let panel_w = pads.width.max(if mode == Mode::Programmer { bits::WIDTH } else { 0.0 });
    let mut w = panel_w + 2.0 * PAD;
    let mut h = toolbar + PAD + display::HEIGHT + PAD + pads.height + PAD;
    if mode == Mode::Programmer {
        h += PROGRAMMER_EXTRA + PAD;
    }
    if tape {
        w += tape::WIDTH;
    }
    Size::new(w, h)
}

/// What's kept between runs: `$XDG_STATE_HOME/aizome/soroban/state.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub mode: Mode,
    pub show_tape: bool,
    pub degrees: bool,
    pub memory: Option<f64>,
    pub radix: u32,
    pub word_bits: u32,
    pub signed: bool,
    pub ans: f64,
    pub int_ans: u64,
    pub tape: Vec<TapeEntry>,
}

impl Default for Saved {
    fn default() -> Saved {
        Saved {
            mode: Mode::Basic,
            show_tape: true,
            degrees: true,
            memory: None,
            radix: 10,
            word_bits: 64,
            signed: true,
            ans: 0.0,
            int_ans: 0,
            tape: Vec::new(),
        }
    }
}

/// Where the state is kept.
pub fn state_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("aizome/soroban/state.json")
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// A key of the keypad.
    Key(Key),
    /// A character typed.
    Typed(char),
    /// The keyboard's key flash is over.
    Unflash(u64),
    SetMode(Mode),
    Radix(u32),
    Word(u32),
    Signed(bool),
    ToggleBit(u32),
    ToggleTape,
    Recall(usize),
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    CopyTape,
    ClearTape,
    /// Ask where to save the tape.
    SaveTape,
    SaveName(String),
    SaveFolder(usize),
    /// Save as the sheet says (twice to replace a file).
    SaveConfirm,
    SaveCancel,
    SaveTapeTo(Option<PathBuf>),
    /// Write the state file (a moment after changes).
    Persist(u64),
    /// Another launch asked for the window.
    Raise,
}

pub struct Soroban {
    pub calc: Calc,
    pub show_tape: bool,
    window: WindowKey,
    /// The key shown pressed because it was typed, and its flash's number.
    flash: Option<(Key, u64)>,
    flash_seq: u64,
    /// Where the state is kept (none in tests).
    state_path: Option<PathBuf>,
    persist_seq: u64,
    /// The listener of the one-instance socket, handed over at launch.
    pub instance: Option<std::os::unix::net::UnixListener>,
    /// Where the tape was last saved.
    pub last_saved: Option<PathBuf>,
    /// The Save Tape sheet, while it's open.
    pub save: Option<SaveSheet>,
    /// The toolbar's height in the theme.
    toolbar: f64,
}

/// The Save Tape sheet: a name and a folder. (The kit's file panel will
/// take its place once it has one.)
pub struct SaveSheet {
    pub key: WindowKey,
    pub name: String,
    pub folder: usize,
    pub folders: Vec<(String, PathBuf)>,
    /// The file is there already: the next Save replaces it.
    pub replacing: bool,
}

impl SaveSheet {
    pub fn path(&self) -> Option<PathBuf> {
        let name = self.name.trim();
        if name.is_empty() || name.contains('/') {
            return None;
        }
        self.folders.get(self.folder).map(|(_, d)| d.join(name))
    }
}

/// Folders to save in: Documents, Desktop and home, those that are there.
fn save_folders() -> Vec<(String, PathBuf)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return vec![("/tmp".into(), PathBuf::from("/tmp"))];
    };
    let mut out = Vec::new();
    for (name, dir) in [("Documents", home.join("Documents")), ("Desktop", home.join("Desktop"))] {
        if dir.is_dir() {
            out.push((name.to_owned(), dir));
        }
    }
    out.push(("Home".to_owned(), home));
    out
}

impl Soroban {
    /// Soroban with its state from `path` (none: start afresh, keep
    /// nothing).
    pub fn new(path: Option<PathBuf>) -> Soroban {
        let saved: Saved = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut calc = Calc::default();
        calc.mode = saved.mode;
        calc.angle = if saved.degrees { Angle::Degrees } else { Angle::Radians };
        calc.memory = saved.memory;
        calc.radix = if matches!(saved.radix, 2 | 8 | 10 | 16) { saved.radix } else { 10 };
        calc.word = aizome_search::calc::int::Word::new(
            if WORDS.contains(&saved.word_bits) { saved.word_bits } else { 64 },
            saved.signed,
        );
        calc.ans = if saved.ans.is_finite() { saved.ans } else { 0.0 };
        calc.int_ans = calc.word.wrap(saved.int_ans);
        calc.tape = saved.tape;
        Soroban {
            calc,
            show_tape: saved.show_tape,
            window: WindowKey::NONE,
            flash: None,
            flash_seq: 0,
            state_path: path,
            persist_seq: 0,
            instance: None,
            last_saved: None,
            save: None,
            toolbar: TOOLBAR,
        }
    }

    fn saved(&self) -> Saved {
        let c = &self.calc;
        Saved {
            mode: c.mode,
            show_tape: self.show_tape,
            degrees: c.angle == Angle::Degrees,
            memory: c.memory,
            radix: c.radix,
            word_bits: c.word.bits,
            signed: c.word.signed,
            ans: c.ans,
            int_ans: c.int_ans,
            tape: c.tape.clone(),
        }
    }

    fn persist_now(&self) {
        let Some(path) = &self.state_path else { return };
        let Ok(json) = serde_json::to_string(&self.saved()) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Written beside and renamed, so a crash leaves the old one whole.
        let tmp = path.with_extension("json.new");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Save a moment from now (a burst of changes saves once).
    fn persist_soon(&mut self, cx: &mut Cx<Msg>) {
        if self.state_path.is_none() {
            return;
        }
        self.persist_seq += 1;
        cx.after(Duration::from_millis(700), Msg::Persist(self.persist_seq));
    }

    fn size(&self) -> Size {
        window_size_in(self.toolbar, self.calc.mode, self.show_tape)
    }

    fn resize(&mut self, cx: &mut Cx<Msg>) {
        self.toolbar = cx.theme().metrics.toolbar;
        cx.request_size(self.window, self.size());
    }

    fn flash(&mut self, key: Key, cx: &mut Cx<Msg>) {
        self.flash_seq += 1;
        self.flash = Some((key, self.flash_seq));
        cx.after(Duration::from_millis(110), Msg::Unflash(self.flash_seq));
    }

    /// The keypad key a typed character stands for, to show it pressed.
    fn key_for(&self, c: char) -> Option<Key> {
        let prog = self.calc.mode == Mode::Programmer;
        Some(match c {
            '0'..='9' => Key::Digit(c as u8 - b'0'),
            'a'..='f' | 'A'..='F' if prog && self.calc.radix == 16 => {
                Key::Digit(c.to_ascii_lowercase() as u8 - b'a' + 10)
            }
            '.' => Key::Point,
            '+' => Key::Add,
            '-' | '−' => Key::Sub,
            '*' | '×' => Key::Mul,
            '/' | '÷' => Key::Div,
            '=' => Key::Equals,
            '%' if prog => Key::Mod,
            '%' => Key::Percent,
            '^' if prog => Key::Xor,
            '^' => Key::Pow,
            '!' if !prog => Key::Fact,
            '~' => Key::Not,
            '&' => Key::And,
            '|' => Key::Or,
            '<' => Key::Shl,
            '>' => Key::Shr,
            '(' => Key::Open,
            ')' => Key::Close,
            'π' => Key::Pi,
            _ => return None,
        })
    }

    fn face(&self, key: Key) -> Face {
        let c = &self.calc;
        let (label, sup) = key.label(c.second);
        let enabled = match key {
            Key::Digit(d) if c.mode == Mode::Programmer => u32::from(d) < c.radix,
            Key::MemRecall => c.memory.is_some(),
            Key::MemClear => c.memory.is_some(),
            Key::Close => !c.fresh && c.open_parens() > 0,
            _ => true,
        };
        let pair = (key == Key::Angle).then_some(("Deg", "Rad", c.angle == Angle::Degrees));
        Face {
            key,
            label,
            sup,
            pair,
            family: key.family(),
            enabled,
            on: key == Key::Second && c.second,
            flashed: self.flash.is_some_and(|(k, _)| k == key),
        }
    }

    fn indicators(&self) -> Vec<Indicator> {
        let c = &self.calc;
        let mut out = Vec::new();
        match c.mode {
            Mode::Basic => {}
            Mode::Scientific => {
                out.push((if c.angle == Angle::Degrees { "DEG" } else { "RAD" }.to_owned(), false));
                if c.second {
                    out.push(("2ND".to_owned(), true));
                }
            }
            Mode::Programmer => {
                out.push((format!("{}-BIT", c.word.bits), false));
                out.push((if c.word.signed { "SIGNED" } else { "UNSIGNED" }.to_owned(), false));
            }
        }
        if c.memory.is_some() && c.mode != Mode::Programmer {
            out.push(("M".to_owned(), true));
        }
        out
    }

    fn toolbar(&self) -> Element<Msg> {
        let names: Vec<&str> = Mode::ALL.iter().map(|m| m.name()).collect();
        let tape_toggle = icon_button(Icon::NotepadText)
            .toolbar()
            .selected(self.show_tape)
            .on_press(Msg::ToggleTape)
            .tooltip(if self.show_tape { "Hide the paper tape (❖T)" } else { "Show the paper tape (❖T)" })
            .label("Paper tape");
        aizome_kit::toolbar(vec![
            dropdown(names, Some(self.calc.mode.index()))
                .compact()
                .on_select(|i| Msg::SetMode(Mode::ALL[i.min(2)]))
                .label("Mode")
                .into(),
            spacer().into(),
            tape_toggle.into(),
        ])
    }

    fn keypads(&self) -> Element<Msg> {
        let mut grids: Vec<Element<Msg>> = Vec::new();
        for g in keypad::grids(self.calc.mode) {
            let mut rows: Vec<Element<Msg>> = Vec::new();
            for r in g.keys.chunks(g.cols) {
                let keys: Vec<Element<Msg>> =
                    r.iter().map(|k| keypad::key(self.face(*k), Msg::Key(*k), k.spoken(self.calc.second))).collect();
                rows.push(aizome_kit::row(keys).spacing(GAP).into());
            }
            grids.push(aizome_kit::column(rows).spacing(GAP).into());
        }
        aizome_kit::row(grids).spacing(GROUP_GAP).align(Align::Start).into()
    }

    fn programmer(&self) -> Element<Msg> {
        let c = &self.calc;
        let value = c.bits();
        let mut radix_rows: Vec<Element<Msg>> = Vec::new();
        for (r, label) in RADIXES {
            let text = aizome_search::calc::int::format(value, r, c.word, true);
            radix_rows.push(bits::radix_row(label, text, c.radix == r, Msg::Radix(r)));
        }
        let word_index = WORDS.iter().position(|w| *w == c.word.bits);
        let words =
            segmented(WORDS.iter().map(|w| w.to_string()), word_index).on_select(|i| Msg::Word(WORDS[i.min(3)]));
        let sign =
            segmented(["Signed", "Unsigned"], if c.word.signed { 0 } else { 1 }).on_select(|i| Msg::Signed(i == 0));
        column![
            row![
                aizome_kit::column(radix_rows).width(Length::Fill),
                column![caption("Word"), words.width(152.0), sign.width(152.0),].spacing(4.0).align(Align::End),
            ]
            .spacing(12.0)
            .align(Align::Start)
            .width(Length::Fill),
            bits::bit_grid(value, c.word.bits, Msg::ToggleBit),
        ]
        .spacing(PAD)
        .into()
    }

    fn tape(&self) -> Element<Msg> {
        let entries: Vec<(String, String)> =
            self.calc.tape.iter().map(|e| (e.expr.clone(), e.result.clone())).collect();
        let has = !entries.is_empty();
        let n = entries.len();
        let tool = |icon: Icon, msg: Msg, tip: &'static str| {
            icon_button(icon).toolbar().on_press_maybe(has.then_some(msg)).tooltip(tip).label(tip)
        };
        container(
            column![
                row![
                    caption("Tape"),
                    spacer(),
                    text(if n == 1 { "1 line".to_owned() } else { format!("{n} lines") }).size(11.0).role(Role::Text3),
                ]
                .width(Length::Fill)
                .height(24.0),
                tape::tape(entries, Msg::Recall),
                row![
                    tool(Icon::Copy, Msg::CopyTape, "Copy Tape"),
                    tool(Icon::Save, Msg::SaveTape, "Save Tape…"),
                    spacer(),
                    tool(Icon::Trash, Msg::ClearTape, "Clear Tape"),
                ]
                .spacing(2.0)
                .width(Length::Fill)
                .height(28.0),
            ]
            .spacing(6.0),
        )
        .padding([4.0, 10.0, 8.0, 10.0])
        .fill(Tone::Chrome)
        .edges(Edge::BevelLo, Sides::LEFT)
        .width(tape::WIDTH)
        .height(Length::Fill)
        .into()
    }
}

/// A caption as the kit sets one (the boards' SHELF), without its padding.
fn caption(s: &str) -> Element<Msg> {
    text(s.to_owned()).mono().size(10.5).letter_spacing(1.2).caps().role(Role::Text3).into()
}

impl Soroban {
    /// The Save Tape sheet's view.
    fn save_view(&self, sheet: &SaveSheet) -> Element<Msg> {
        let names: Vec<String> = sheet.folders.iter().map(|(n, _)| n.clone()).collect();
        let error = if sheet.replacing {
            Some(format!("“{}” is there already. Save again to replace it.", sheet.name.trim()))
        } else {
            None
        };
        let width = (self.size().width - 24.0).min(320.0);
        column![
            text("Save Tape").semibold().size(15.0),
            text_field(&sheet.name).id("save-name").on_change(Msg::SaveName).on_submit(Msg::SaveConfirm).error(error),
            row![
                text("In").role(Role::Text2),
                dropdown(names, Some(sheet.folder)).on_select(Msg::SaveFolder).width(Length::Fill),
            ]
            .spacing(8.0)
            .width(Length::Fill),
            row![
                spacer(),
                button("Cancel").cancel().on_press(Msg::SaveCancel),
                button(if sheet.replacing { "Replace" } else { "Save" })
                    .default()
                    .on_press_maybe(sheet.path().map(|_| Msg::SaveConfirm)),
            ]
            .spacing(8.0)
            .width(Length::Fill),
        ]
        .spacing(12.0)
        .padding(16.0)
        .width(width)
        .into()
    }
}

/// A key press, as the calculator hears it (or not: Tab, arrows and
/// shortcuts go on to the focus and the menus).
pub fn key_message(k: &KeyEvent) -> Option<Msg> {
    let m = k.modifiers;
    if m.super_key() || m.control() || m.alt() {
        return None;
    }
    match &k.key {
        KeyName::Named(NamedKey::Enter) => Some(Msg::Key(Key::Equals)),
        KeyName::Named(NamedKey::Backspace) => Some(Msg::Key(Key::Back)),
        KeyName::Named(NamedKey::Escape | NamedKey::Delete | NamedKey::Clear) => Some(Msg::Key(Key::Clear)),
        KeyName::Character(s) => {
            let text = k.text.as_deref().unwrap_or(s);
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if !c.is_control() => Some(Msg::Typed(c)),
                _ => None,
            }
        }
        _ => None,
    }
}

impl Program for Soroban {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Soroban", "Calculator").icon(Icon::Calculator).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.toolbar = cx.theme().metrics.toolbar;
        let size = self.size();
        self.window = cx.open_window(WindowSpec::new("Soroban").size(size.width, size.height).resizable(false));
        cx.focus("keyboard");
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, cx.stream(), || Msg::Raise);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Key(k) => {
                let changed = self.calc.press(k);
                if changed || k == Key::Equals || k == Key::Angle {
                    self.persist_soon(cx);
                }
            }
            Msg::Typed(c) => {
                if let Some(k) = self.key_for(c) {
                    self.flash(k, cx);
                }
                let before = self.calc.tape.len();
                self.calc.type_char(c);
                if self.calc.tape.len() != before {
                    self.persist_soon(cx);
                }
            }
            Msg::Unflash(seq) => {
                if self.flash.is_some_and(|(_, s)| s == seq) {
                    self.flash = None;
                }
            }
            Msg::SetMode(mode) => {
                if mode != self.calc.mode {
                    self.calc.set_mode(mode);
                    self.resize(cx);
                    self.persist_soon(cx);
                }
            }
            Msg::Radix(r) => {
                self.calc.set_radix(r);
                self.persist_soon(cx);
            }
            Msg::Word(bits) => {
                let signed = self.calc.word.signed;
                self.calc.set_word(bits, signed);
                self.persist_soon(cx);
            }
            Msg::Signed(signed) => {
                let bits = self.calc.word.bits;
                self.calc.set_word(bits, signed);
                self.persist_soon(cx);
            }
            Msg::ToggleBit(i) => self.calc.toggle_bit(i),
            Msg::ToggleTape => {
                self.show_tape = !self.show_tape;
                self.resize(cx);
                self.persist_soon(cx);
            }
            Msg::Recall(i) => self.calc.recall(i),
            Msg::Copy => {
                let text = self.calc.copy_text();
                cx.set_clipboard_text(&text);
            }
            Msg::Cut => {
                let text = self.calc.copy_text();
                cx.set_clipboard_text(&text);
                self.calc.press(Key::Clear);
            }
            Msg::Paste => {
                if let Some(text) = cx.clipboard_text() {
                    self.calc.paste(&text);
                }
            }
            Msg::Undo => self.calc.undo(),
            Msg::Redo => self.calc.redo(),
            Msg::CopyTape => {
                let text = self.calc.tape_text();
                cx.set_clipboard_text(&text);
            }
            Msg::ClearTape => {
                self.calc.clear_tape();
                self.persist_soon(cx);
            }
            Msg::SaveTape => {
                if self.save.is_none() && !self.calc.tape.is_empty() {
                    let key = cx.show_sheet(self.window);
                    let folders = save_folders();
                    self.save = Some(SaveSheet { key, name: "Tape.txt".into(), folder: 0, folders, replacing: false });
                    cx.focus("save-name");
                }
            }
            Msg::SaveName(name) => {
                if let Some(s) = &mut self.save {
                    s.name = name;
                    s.replacing = false;
                }
            }
            Msg::SaveFolder(i) => {
                if let Some(s) = &mut self.save {
                    s.folder = i;
                    s.replacing = false;
                }
            }
            Msg::SaveConfirm => {
                let Some(s) = &mut self.save else { return };
                let Some(path) = s.path() else { return };
                if path.exists() && !s.replacing {
                    s.replacing = true;
                    return;
                }
                let key = s.key;
                self.save = None;
                cx.close_window(key);
                cx.send(Msg::SaveTapeTo(Some(path)));
            }
            Msg::SaveCancel => {
                if let Some(s) = self.save.take() {
                    cx.close_window(s.key);
                }
            }
            Msg::SaveTapeTo(Some(path)) => match std::fs::write(&path, self.calc.tape_text()) {
                Ok(()) => self.last_saved = Some(path),
                Err(e) => {
                    let alert = Alert::new("The tape couldn't be saved", format!("{}: {e}", path.display()));
                    cx.alert(self.window, alert);
                }
            },
            Msg::SaveTapeTo(None) => {}
            Msg::Persist(seq) => {
                if seq == self.persist_seq {
                    self.persist_now();
                }
            }
            Msg::Raise => cx.activate(self.window),
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if let Some(s) = self.save.as_ref().filter(|s| s.key == window) {
            return self.save_view(s);
        }
        let c = &self.calc;
        let mut panel: Vec<Element<Msg>> = vec![display::display(c.shown(), self.indicators())];
        if c.mode == Mode::Programmer {
            panel.push(self.programmer());
        }
        panel.push(self.keypads());
        let panel = aizome_kit::column(panel).spacing(PAD).padding(PAD).width(Length::Fill).height(Length::Fill);
        let body: Element<Msg> = if self.show_tape {
            row![panel, self.tape()].align(Align::Start).width(Length::Fill).height(Length::Fill).into()
        } else {
            panel.into()
        };
        keypad::keyboard(column![self.toolbar(), body].width(Length::Fill).height(Length::Fill), key_message)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let c = &self.calc;
        let has_tape = !c.tape.is_empty();
        let prog = c.mode == Mode::Programmer;
        let file = Menu::new("File")
            .item(MenuItem::new("Save Tape…", Msg::SaveTape).shortcut(Shortcut::cmd('S')).enabled(has_tape))
            .item(MenuItem::new("Copy Tape", Msg::CopyTape).shortcut(Shortcut::cmd('C').shift()).enabled(has_tape))
            .item(MenuItem::new("Clear Tape", Msg::ClearTape).shortcut(Shortcut::cmd('K')).enabled(has_tape))
            .separator()
            .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
        let edit = Menu::new("Edit")
            .item(MenuItem::new("Undo", Msg::Undo).shortcut(Shortcut::cmd('Z')).enabled(c.can_undo()))
            .item(MenuItem::new("Redo", Msg::Redo).shortcut(Shortcut::cmd('Z').shift()).enabled(c.can_redo()))
            .separator()
            .item(MenuItem::new("Cut", Msg::Cut).role(MenuRole::Cut).shortcut(Shortcut::cmd('X')))
            .item(MenuItem::new("Copy", Msg::Copy).role(MenuRole::Copy).shortcut(Shortcut::cmd('C')))
            .item(MenuItem::new("Paste", Msg::Paste).role(MenuRole::Paste).shortcut(Shortcut::cmd('V')))
            .separator()
            .item(MenuItem::new("Clear", Msg::Key(Key::Clear)).shortcut(Shortcut::key(ShortcutKey::Escape)));
        let mut view = Menu::new("View");
        for (i, m) in Mode::ALL.iter().enumerate() {
            let digit = char::from(b'1' + i as u8);
            view = view
                .item(MenuItem::new(m.name(), Msg::SetMode(*m)).shortcut(Shortcut::cmd(digit)).checked(c.mode == *m));
        }
        view = view
            .separator()
            .item(MenuItem::new("Paper Tape", Msg::ToggleTape).shortcut(Shortcut::cmd('T')).checked(self.show_tape));
        let mut calc = Menu::new("Calculate")
            .item(MenuItem::new("Equals", Msg::Key(Key::Equals)).shortcut(Shortcut::key(ShortcutKey::Enter)))
            .item(MenuItem::new("Delete", Msg::Key(Key::Back)).shortcut(Shortcut::key(ShortcutKey::Backspace)))
            .item(MenuItem::new("Change Sign", Msg::Key(Key::Negate)))
            .separator();
        if prog {
            let mut base = Menu::new("Radix");
            for (r, name, k) in [(16, "Hexadecimal", 'H'), (10, "Decimal", 'D'), (8, "Octal", 'O'), (2, "Binary", 'B')]
            {
                base = base
                    .item(MenuItem::new(name, Msg::Radix(r)).shortcut(Shortcut::cmd(k).shift()).checked(c.radix == r));
            }
            let mut word = Menu::new("Word Size");
            for w in WORDS {
                word = word.item(MenuItem::new(format!("{w} Bits"), Msg::Word(w)).checked(c.word.bits == w));
            }
            word = word.separator().item(MenuItem::new("Signed", Msg::Signed(!c.word.signed)).checked(c.word.signed));
            calc = calc.submenu(base).submenu(word);
        } else {
            let mem = Menu::new("Memory")
                .item(MenuItem::new("Memory Clear", Msg::Key(Key::MemClear)).enabled(c.memory.is_some()))
                .item(MenuItem::new("Memory Add", Msg::Key(Key::MemAdd)))
                .item(MenuItem::new("Memory Subtract", Msg::Key(Key::MemSub)))
                .item(MenuItem::new("Memory Recall", Msg::Key(Key::MemRecall)).enabled(c.memory.is_some()));
            calc = calc
                .item(
                    MenuItem::new("Degrees", Msg::Key(Key::Angle))
                        .checked(c.angle == Angle::Degrees)
                        .shortcut(Shortcut::cmd('D')),
                )
                .item(MenuItem::new("Second Functions", Msg::Key(Key::Second)).checked(c.second))
                .submenu(mem);
        }
        MenuBar::standard(&self.info()).replace("Edit", edit).with_file_menu(file).with_menu(view).with_menu(calc)
    }

    fn close_requested(&mut self, _window: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        self.persist_now();
        true
    }
}

/// The value the display shows, as a number (for tests).
pub fn shown_value(s: &Soroban) -> Option<Num> {
    if s.calc.fresh { Some(s.calc.answer()) } else { s.calc.evaluate().ok() }
}
