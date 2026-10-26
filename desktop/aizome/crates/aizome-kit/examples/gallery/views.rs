//! The gallery's program, shared with the golden-image tests.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::theme::{SizeChoice, ThemeChoice};
use aizome_kit::{Appearance, EditorAction, EditorContent, LineStyle};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Controls,
    Fields,
    Lists,
    Settings,
    Editor,
}

pub const PAGES: [(Page, Icon, &str); 5] = [
    (Page::Controls, Icon::SlidersHorizontal, "Controls"),
    (Page::Fields, Icon::TextCursorInput, "Text & Fields"),
    (Page::Lists, Icon::ListTree, "Lists & Tables"),
    (Page::Settings, Icon::Settings, "Settings Pane"),
    (Page::Editor, Icon::SquarePen, "Editor"),
];

#[derive(Clone, Debug)]
pub enum Msg {
    Page(Page),
    Theme(usize),
    Toggle(usize, bool),
    Check(usize, bool),
    Radio(u8),
    Slider(f64),
    Stepper(f64),
    Segment(usize),
    Pick(usize),
    Name(String),
    Search(String),
    Secret(String),
    Select(Vec<usize>),
    Sort(usize, bool),
    Tree(u64, bool),
    TreeSelect(Vec<u64>),
    Edit(EditorAction),
    Pressed,
    Popover(bool),
    /// `GALLERY_DEMO=menu` (or `popover`, `alert`) shows one a moment after
    /// launch, for screenshots under a headless compositor.
    Demo,
}

pub struct Gallery {
    pub page: Page,
    pub theme: usize,
    pub toggles: [bool; 4],
    pub checks: [bool; 3],
    pub radio: u8,
    pub slider: f64,
    pub stepper: f64,
    pub segment: usize,
    pub pick: usize,
    pub name: String,
    pub search: String,
    pub secret: String,
    pub files: Rc<Vec<(String, u64)>>,
    pub selected: Vec<usize>,
    pub sort: (usize, bool),
    pub open: Arc<HashSet<u64>>,
    pub tree_selected: Vec<u64>,
    pub content: EditorContent,
    pub presses: u32,
    pub popover: bool,
    pub window: Option<WindowKey>,
    /// A row the lists scroll to show (the scrolling benchmark).
    pub reveal: Option<usize>,
}

impl Default for Gallery {
    fn default() -> Gallery {
        let files = (0..10_000).map(|i| (format!("file-{i:05}.md"), (i as u64 * 7919) % 100_000)).collect();
        Gallery {
            page: Page::Controls,
            theme: 0,
            toggles: [true, false, true, false],
            checks: [true, false, true],
            radio: 1,
            slider: 0.62,
            stepper: 13.0,
            segment: 0,
            pick: 0,
            name: "Callen".into(),
            search: String::new(),
            secret: "hunter2".into(),
            files: Rc::new(files),
            selected: vec![2],
            sort: (0, true),
            open: Arc::new([1u64].into_iter().collect()),
            tree_selected: vec![3],
            content: EditorContent::new(EDITOR_TEXT),
            presses: 0,
            popover: false,
            window: None,
            reveal: None,
        }
    }
}

pub const EDITOR_TEXT: &str = "## Scale\n\nOperations stay in points. Each window renders at its\noutput's scale: the integer one wherever it can, the\nfractional one only when asked. Every surface gets a\nbuffer of `points × scale` pixels, so the compositor maps\nbuffer pixels to screen pixels one to one.\n\n### Damage\nA canvas fills only the pixels whose centres its\ndamage contains, so neighbouring fills share edges.\n\n- Clock tick: one rect, the clock's own.\n- Caret blink: two points wide, one line tall.\n";

pub fn markdown() -> aizome_kit::Highlighter {
    Arc::new(|_line, text: &str| {
        let mut out = Vec::new();
        if text.starts_with('#') {
            out.push(LineStyle { range: 0..text.len(), span: Span::new("").role(Role::Accent).weight(600.0) });
        }
        let mut from = 0;
        while let Some(a) = text[from..].find('`') {
            let a = from + a;
            let Some(b) = text[a + 1..].find('`') else { break };
            out.push(LineStyle { range: a..a + b + 2, span: Span::new("").role(Role::Success) });
            from = a + b + 2;
        }
        if text.starts_with("- ") {
            out.push(LineStyle { range: 0..2, span: Span::new("").role(Role::Violet) });
        }
        out
    })
}

impl Gallery {
    pub fn update_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Page(p) => self.page = p,
            Msg::Theme(t) => self.theme = t,
            Msg::Toggle(i, b) => self.toggles[i] = b,
            Msg::Check(i, b) => self.checks[i] = b,
            Msg::Radio(r) => self.radio = r,
            Msg::Slider(v) => self.slider = v,
            Msg::Stepper(v) => self.stepper = v,
            Msg::Segment(s) => self.segment = s,
            Msg::Pick(p) => self.pick = p,
            Msg::Name(s) => self.name = s,
            Msg::Search(s) => self.search = s,
            Msg::Secret(s) => self.secret = s,
            Msg::Select(s) => self.selected = s,
            Msg::Sort(c, a) => {
                self.sort = (c, a);
                let mut v = (*self.files).clone();
                v.sort_by(|x, y| {
                    let o = if c == 0 { x.0.cmp(&y.0) } else { x.1.cmp(&y.1) };
                    if a { o } else { o.reverse() }
                });
                self.files = Rc::new(v);
            }
            Msg::Tree(id, on) => {
                let mut o = (*self.open).clone();
                if on {
                    o.insert(id);
                } else {
                    o.remove(&id);
                }
                self.open = Arc::new(o);
            }
            Msg::TreeSelect(s) => self.tree_selected = s,
            Msg::Edit(a) => {
                self.content.perform(a);
            }
            Msg::Pressed => self.presses += 1,
            Msg::Popover(b) => self.popover = b,
            Msg::Demo => {}
        }
    }

    pub fn appearance(&self) -> Appearance {
        let theme = [ThemeChoice::Night, ThemeChoice::Day, ThemeChoice::HighContrast][self.theme.min(2)];
        Appearance { theme, ..Appearance::default() }
    }

    fn heading(t: &str) -> Element<Msg> {
        text(t).size(15.0).semibold().into()
    }

    /// Controls in their states.
    pub fn controls(&self) -> Element<Msg> {
        column![
            Self::heading("Buttons"),
            aizome_kit::row![
                button("Default").on_press(Msg::Pressed),
                button("Primary").primary().on_press(Msg::Pressed).id("primary"),
                button("Delete").destructive().on_press(Msg::Pressed),
                button("Disabled"),
                button("Add").icon(Icon::Plus).on_press(Msg::Pressed),
                icon_button(Icon::Settings).on_press(Msg::Pressed).tooltip("Settings"),
            ]
            .spacing(8.0),
            aizome_kit::row![
                button("B").toolbar().weight(800.0).selected(true).on_press(Msg::Pressed),
                button("I").toolbar().italic().on_press(Msg::Pressed),
                button("</>").toolbar().mono().on_press(Msg::Pressed),
                dropdown(["Markdown", "Plain Text", "Rust"], self.pick).on_select(Msg::Pick).compact(),
                popover(
                    button("Popover…").on_press(Msg::Popover(true)),
                    column![text("A popover").semibold(), text("Floating, with the hard shadow.").role(Role::Text3)]
                        .spacing(4.0)
                        .padding(12.0),
                    self.popover
                )
                .on_dismiss(Msg::Popover(false)),
            ]
            .spacing(6.0),
            Self::heading("Choices"),
            aizome_kit::row![
                toggle(self.toggles[0]).on_toggle(|b| Msg::Toggle(0, b)),
                toggle(self.toggles[1]).on_toggle(|b| Msg::Toggle(1, b)).id("toggle-off"),
                toggle(true),
                space(16.0),
                checkbox("Checked", self.checks[0]).on_toggle(|b| Msg::Check(0, b)),
                checkbox("Unchecked", self.checks[1]).on_toggle(|b| Msg::Check(1, b)),
                checkbox("Disabled", true),
            ]
            .spacing(12.0),
            aizome_kit::row![
                radio("Night", 0u8, Some(self.radio)).on_select(Msg::Radio),
                radio("Day", 1u8, Some(self.radio)).on_select(Msg::Radio),
                radio("Auto", 2u8, Some(self.radio)).on_select(Msg::Radio),
                space(16.0),
                segmented(["Small", "Medium", "Large"], self.segment).on_select(Msg::Segment),
                stepper(self.stepper).range(8.0..=72.0).suffix(" pt").on_change(Msg::Stepper),
            ]
            .spacing(12.0),
            aizome_kit::row![
                slider(self.slider, 0.0..=1.0).ticks(5).on_change(Msg::Slider).width(240.0),
                slider(0.3, 0.0..=1.0).width(160.0)
            ]
            .spacing(16.0),
            Self::heading("Progress and status"),
            aizome_kit::row![
                progress(self.slider).width(200.0),
                progress_indeterminate().width(160.0),
                meter(vec![0.2, 0.5, 0.9, 0.35, 0.6, 0.1, 0.75, 0.4]),
                meter(vec![0.62]).role(Role::Accent)
            ]
            .spacing(16.0),
            aizome_kit::row![
                badge("3"),
                badge("new").role(Role::Accent),
                keycap("❖"),
                keycap("⇧"),
                keycap("⌥"),
                keycap("⌃"),
                keycap("F"),
                text("❖⇧Z").mono().role(Role::Text3)
            ]
            .spacing(8.0),
            Self::heading("Icons"),
            aizome_kit::widgets::layout::row(
                [
                    Icon::Wifi,
                    Icon::Bluetooth,
                    Icon::Volume2,
                    Icon::BatteryMedium,
                    Icon::Sun,
                    Icon::Moon,
                    Icon::Monitor,
                    Icon::Keyboard,
                    Icon::Printer,
                    Icon::Lock,
                    Icon::Bell,
                    Icon::Search,
                    Icon::Folder,
                    Icon::HardDrive,
                    Icon::Trash,
                    Icon::Terminal,
                    Icon::Calendar,
                    Icon::Clock,
                    Icon::Calculator,
                    Icon::CloudSun,
                    Icon::StickyNote,
                    Icon::ChartLine,
                    Icon::Cpu,
                    Icon::Palette
                ]
                .into_iter()
                .map(|i| icon(i).size(16.0).role(Role::Text2).into())
                .collect()
            )
            .spacing(10.0),
        ]
        .spacing(12.0)
        .padding(20.0)
        .into()
    }

    /// Text and fields.
    pub fn fields(&self) -> Element<Msg> {
        column![
            Self::heading("Text"),
            text("Archivo, the interface's type, at 13pt in text.").into_el(),
            text("Secondary text, in text2.").role(Role::Text2),
            text("Tertiary text and captions, in text3.").role(Role::Text3),
            text("IBM Plex Mono for code and numbers: 0123456789").mono(),
            rich_text([Span::new("Accent ").role(Role::Accent), Span::new("Danger ").role(Role::Danger), Span::new("Success ").role(Role::Success), Span::new("Warning ").role(Role::Warning), Span::new("bold ").bold(), Span::new("italic").italic()]),
            text("A paragraph that wraps: Aizome 藍染 is Sekka's desktop, built on Sidestep's native toolkit. Its look is square corners, 1pt bevels and hard shadows on what floats.").wrap().max_width(520.0),
            Self::heading("Fields"),
            aizome_kit::form_section(
                "",
                vec![
                    aizome_kit::form_row("Name", text_field(&self.name).placeholder("Your name").on_change(Msg::Name).width(260.0).id("name")).into(),
                    aizome_kit::form_row("Search", search_field(&self.search).on_change(Msg::Search).width(260.0)).into(),
                    aizome_kit::form_row("Password", text_field(&self.secret).password().on_change(Msg::Secret).width(260.0)).into(),
                    aizome_kit::form_row("Port", text_field("80x").error(Some("A number, please")).on_change(Msg::Name).width(260.0)).into(),
                    aizome_kit::form_row("Disabled", text_field("Can't touch this").enabled(false).width(260.0)).into(),
                ],
            ),
        ]
        .spacing(10.0)
        .padding(20.0)
        .into()
    }

    /// A virtual list, a table, an outline.
    pub fn lists(&self) -> Element<Msg> {
        let files = self.files.clone();
        let names = self.files.clone();
        let rows = self.files.clone();
        let items = Arc::new(vec![
            TreeItem::branch(
                1,
                "home",
                vec![
                    TreeItem::branch(
                        2,
                        "callen",
                        vec![
                            TreeItem::leaf(3, "work").icon(Icon::Folder).detail("41.2 GB"),
                            TreeItem::leaf(4, "Downloads").icon(Icon::Download).detail("3.1 GB"),
                        ],
                    )
                    .icon(Icon::House)
                    .detail("44.3 GB"),
                ],
            )
            .icon(Icon::HardDrive)
            .detail("44.3 GB"),
            TreeItem::branch(5, "nix", vec![TreeItem::leaf(6, "store").icon(Icon::Package).detail("88.0 GB")])
                .icon(Icon::HardDrive)
                .detail("88.0 GB"),
        ]);
        aizome_kit::row![
            container(
                virtual_list(files.len(), 24.0, move |i| aizome_kit::row![
                    icon(Icon::FileText).size(14.0),
                    text(files[i].0.clone())
                ]
                .spacing(8.0)
                .padding([0.0, 10.0]))
                .selected(self.selected.iter().copied())
                .on_select(Msg::Select)
                .multiple()
                .type_select(move |i| names[i].0.clone())
                .reveal(self.reveal.unwrap_or(0))
            )
            .border(Edge::Line)
            .padding(1.0)
            .width(170.0)
            .height(Fill),
            container(
                table(
                    vec![
                        Column::new("Name").width(130.0).sortable(),
                        Column::new("Size").width(80.0).align_end().sortable()
                    ],
                    rows.len(),
                    move |r, c| {
                        if c == 0 {
                            text(rows[r].0.clone())
                        } else {
                            text(format!("{} KB", rows[r].1)).mono().tabular().role(Role::Text2)
                        }
                    }
                )
                .selected(self.selected.iter().copied())
                .on_select(Msg::Select)
                .on_sort(Msg::Sort)
                .sorted(self.sort.0, self.sort.1)
                .reveal(self.reveal.unwrap_or(0))
            )
            .border(Edge::Line)
            .padding(1.0)
            .width(Fill)
            .height(Fill),
            container(
                tree(items)
                    .expanded(self.open.clone())
                    .on_toggle(Msg::Tree)
                    .selected(self.tree_selected.iter().copied())
                    .on_select(Msg::TreeSelect)
            )
            .border(Edge::Line)
            .padding(1.0)
            .width(190.0)
            .height(Fill),
        ]
        .spacing(12.0)
        .padding(20.0)
        .height(Fill)
        .into()
    }

    /// A Settings-like pane.
    pub fn settings(&self) -> Element<Msg> {
        scroll(
            column![
                text("Appearance").size(20.0).semibold(),
                aizome_kit::form_section(
                    "Look",
                    vec![
                        aizome_kit::form_row(
                            "Theme",
                            segmented(["Night", "Day", "High Contrast"], self.theme).on_select(Msg::Theme)
                        )
                        .into(),
                        aizome_kit::form_row(
                            "Focus colour",
                            segmented(["Amber", "Cyan", "Rose", "Lime"], 0).on_select(|_| Msg::Pressed)
                        )
                        .into(),
                        aizome_kit::form_row(
                            "Size",
                            dropdown(["Small", "Medium", "Large"], self.segment).on_select(Msg::Segment)
                        )
                        .description("Scales type and controls, not pixels.")
                        .into(),
                        aizome_kit::form_row(
                            "Reduced motion",
                            toggle(self.toggles[1]).on_toggle(|b| Msg::Toggle(1, b))
                        )
                        .into(),
                    ],
                ),
                aizome_kit::form_section(
                    "Network",
                    vec![
                        aizome_kit::form_row("Wi-Fi", toggle(self.toggles[0]).on_toggle(|b| Msg::Toggle(0, b)))
                            .icon(Icon::Wifi)
                            .into(),
                        aizome_kit::form_row("Bluetooth", toggle(self.toggles[2]).on_toggle(|b| Msg::Toggle(2, b)))
                            .icon(Icon::Bluetooth)
                            .into(),
                        aizome_kit::form_row(
                            "Volume",
                            slider(self.slider, 0.0..=1.0).on_change(Msg::Slider).width(200.0)
                        )
                        .icon(Icon::Volume2)
                        .into(),
                    ],
                ),
            ]
            .spacing(16.0)
            .padding(20.0)
            .max_width(640.0),
        )
        .into()
    }

    /// An editor, as the board's.
    pub fn editor(&self) -> Element<Msg> {
        let (line, col) = self.content.cursor_position();
        column![
            aizome_kit::toolbar(vec![
                dropdown(["Markdown", "Plain Text", "Rust"], self.pick).on_select(Msg::Pick).compact().into(),
                aizome_kit::toolbar_separator(),
                button("B").toolbar().weight(800.0).on_press(Msg::Pressed).into(),
                button("I").toolbar().italic().on_press(Msg::Pressed).into(),
                button("</>").toolbar().mono().on_press(Msg::Pressed).into(),
                spacer().into(),
                aizome_kit::status_text(format!("Ln {}, Col {} · UTF-8 · LF", line + 1, col + 1)),
            ]),
            text_editor(&self.content).on_action(Msg::Edit).line_numbers(true).highlighter(markdown(), 0).id("editor"),
        ]
        .into()
    }

    pub fn page_view(&self) -> Element<Msg> {
        match self.page {
            Page::Controls => scroll(self.controls()).into(),
            Page::Fields => scroll(self.fields()).into(),
            Page::Lists => self.lists(),
            Page::Settings => self.settings(),
            Page::Editor => self.editor(),
        }
    }
}

trait IntoEl {
    fn into_el(self) -> Element<Msg>;
}

impl IntoEl for aizome_kit::text::Text<Msg> {
    fn into_el(self) -> Element<Msg> {
        self.into()
    }
}

impl Program for Gallery {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Gallery", "Gallery", "Widget Gallery").icon(Icon::LayoutGrid)
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let w = cx.open_window(WindowSpec::new("Gallery").size(1100.0, 720.0).min_size(720.0, 480.0));
        self.window = Some(w);
        if std::env::var_os("GALLERY_DEMO").is_some() {
            cx.after(std::time::Duration::from_millis(1200), Msg::Demo);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        if let (Msg::Demo, Some(w)) = (&msg, self.window) {
            match std::env::var("GALLERY_DEMO").as_deref() {
                // Keep changing something, for frame statistics: a switch
                // on Controls, a list's selection on Lists, typing on the
                // Editor.
                Ok("bench-toggle") => {
                    self.toggles[1] = !self.toggles[1];
                    cx.after(std::time::Duration::from_millis(20), Msg::Demo);
                }
                Ok("bench-select") => {
                    self.page = Page::Lists;
                    let next = self.selected.first().map_or(0, |s| (s + 1) % 30);
                    self.selected = vec![next];
                    cx.after(std::time::Duration::from_millis(20), Msg::Demo);
                }
                // Scroll the list and the table two rows at a time, and
                // the editor a line at a time through a long text (its caret on the
                // last line).
                Ok("bench-scroll") => {
                    self.page = Page::Lists;
                    self.reveal = Some((self.reveal.unwrap_or(40) + 2) % 10_000);
                    cx.after(std::time::Duration::from_millis(20), Msg::Demo);
                }
                Ok("bench-scroll-editor") => {
                    if self.page != Page::Editor {
                        self.page = Page::Editor;
                        let line = "Operations stay in points; each window renders at its output's scale.\n";
                        self.content.set_text(&line.repeat(4000));
                    }
                    let rope = self.content.rope();
                    let next = (rope.char_to_line(self.content.selection().head) + 1).max(60) % rope.len_lines();
                    let at = rope.line_to_char(next);
                    self.content.set_selection(aizome_kit::Selection::caret(at));
                    cx.after(std::time::Duration::from_millis(20), Msg::Demo);
                }
                Ok("bench-type") => {
                    if self.page != Page::Editor {
                        self.page = Page::Editor;
                        self.content.set_selection(aizome_kit::Selection::caret(120));
                    }
                    let a = if self.presses % 20 == 19 {
                        EditorAction::Backspace(aizome_kit::Granularity::Word)
                    } else {
                        EditorAction::Insert("x".into())
                    };
                    self.presses += 1;
                    self.content.perform(a);
                    cx.after(std::time::Duration::from_millis(20), Msg::Demo);
                }
                Ok("popover") => self.popover = true,
                Ok("alert") => cx.alert(
                    w,
                    Alert::new("Delete “notes.md”?", "It goes to the Trash.")
                        .destructive_button("Delete", Msg::Pressed)
                        .cancel_button("Cancel")
                        .default_button("Keep", Msg::Pressed),
                ),
                _ => {
                    let view = Menu::new("View")
                        .items(PAGES.iter().enumerate().map(|(n, (p, i, name))| {
                            MenuItem::new(*name, Msg::Page(*p))
                                .icon(*i)
                                .checked(self.page == *p)
                                .shortcut(Shortcut::cmd(char::from(b'1' + n as u8)))
                        }))
                        .separator()
                        .submenu(
                            Menu::new("Theme").items(
                                ["Night", "Day", "High Contrast"]
                                    .iter()
                                    .enumerate()
                                    .map(|(n, t)| MenuItem::new(*t, Msg::Theme(n)).checked(self.theme == n)),
                            ),
                        );
                    cx.show_menu(
                        w,
                        Rect::new(380.0, 120.0, 381.0, 121.0),
                        view,
                        MenuStyle::at_point().titled("View").on_tear_off(Msg::Pressed),
                    );
                }
            }
            return;
        }
        let theme = matches!(msg, Msg::Theme(_));
        self.update_msg(msg);
        if theme {
            cx.set_appearance(self.appearance());
        }
    }

    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let mut side: Vec<Element<Msg>> = vec![aizome_kit::caption("Gallery")];
        for (p, i, name) in PAGES {
            side.push(aizome_kit::sidebar_item(i, name, self.page == p).on_press(Msg::Page(p)).key(name).into());
        }
        side.push(spacer().into());
        side.push(container(segmented(["Night", "Day", "HC"], self.theme).on_select(Msg::Theme)).padding(10.0).into());
        aizome_kit::sidebar_layout(aizome_kit::widgets::layout::column(side).height(Fill), self.page_view()).into()
    }

    fn menus(&self) -> MenuBar<Msg> {
        MenuBar::standard(&self.info()).with_menu(
            Menu::new("View")
                .items(PAGES.iter().enumerate().map(|(n, (p, i, name))| {
                    MenuItem::new(*name, Msg::Page(*p))
                        .icon(*i)
                        .checked(self.page == *p)
                        .shortcut(Shortcut::cmd(char::from(b'1' + n as u8)))
                }))
                .separator()
                .submenu(
                    Menu::new("Theme").items(
                        ["Night", "Day", "High Contrast"]
                            .iter()
                            .enumerate()
                            .map(|(n, t)| MenuItem::new(*t, Msg::Theme(n)).checked(self.theme == n)),
                    ),
                ),
        )
    }
}

#[allow(dead_code)]
pub fn sizes() -> [SizeChoice; 3] {
    [SizeChoice::Small, SizeChoice::Medium, SizeChoice::Large]
}
