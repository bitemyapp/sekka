//! Hotaru's Settings window (❖,): the configuration file's choices as
//! Settei shows the desktop's, a sidebar of pages and forms, each change
//! reaching every pane as it's made and saved to `hotaru.toml`.

use std::sync::Arc;

use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::kurbo::BezPath;
use aizome_kit::sidestep_ui::{Key, NamedKey, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{column, row};
use hotaru_core::{Rgb, TerminalColors};

use crate::app::{Hotaru, Msg};
use crate::settings::{self, Access, Action, CopyFormat, CursorStyle};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    General,
    Text,
    Appearance,
    Shortcuts,
}

impl Page {
    const ALL: [Page; 4] = [Page::General, Page::Text, Page::Appearance, Page::Shortcuts];

    fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Text => "Text",
            Page::Appearance => "Appearance",
            Page::Shortcuts => "Shortcuts",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Page::General => Icon::SquareTerminal,
            Page::Text => Icon::Type,
            Page::Appearance => Icon::Palette,
            Page::Shortcuts => Icon::Keyboard,
        }
    }
}

/// A status line under the page: what was just done, or why it wasn't.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

impl Notice {
    pub fn done(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: false }
    }
    pub fn failed(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: true }
    }
}

const PAGE_WIDTH: f64 = 560.0;
pub const RECORDER: &str = "shortcut-recorder";

const SCROLLBACK: [usize; 4] = [1_000, 10_000, 100_000, 1_000_000];
const COPY_FORMATS: [(CopyFormat, &str); 4] = [
    (CopyFormat::Mixed, "Plain Text and HTML"),
    (CopyFormat::Plain, "Plain Text"),
    (CopyFormat::Vt, "Text with Terminal Colours"),
    (CopyFormat::Html, "HTML"),
];
pub const WEIGHTS: [(u16, &str); 5] =
    [(300, "Light"), (400, "Regular"), (500, "Medium"), (600, "Semibold"), (700, "Bold")];
const CURSORS: [(CursorStyle, &str); 3] =
    [(CursorStyle::Block, "Block"), (CursorStyle::Bar, "Bar"), (CursorStyle::Underline, "Underline")];
const ACCESS: [(Access, &str); 3] = [(Access::Ask, "Ask"), (Access::Allow, "Allow"), (Access::Deny, "Never")];

pub fn view(app: &Hotaru) -> Element<Msg> {
    let mut side: Vec<Element<Msg>> = vec![aizome_kit::caption("Hotaru")];
    for page in Page::ALL {
        side.push(
            sidebar_item(page.icon(), page.title(), app.settings_page == page)
                .on_press(Msg::SettingsPage(page))
                .key(page.title())
                .into(),
        );
    }
    let body = match app.settings_page {
        Page::General => general(app),
        Page::Text => text_page(app),
        Page::Appearance => appearance(app),
        Page::Shortcuts => shortcuts(app),
    };
    let mut page = vec![text(app.settings_page.title()).size(20.0).semibold().into()];
    if app.settings_managed {
        page.push(
            row![
                icon(Icon::Lock).size(14.0).role(Role::Warning),
                text(
                    "hotaru.toml is a link to a file managed elsewhere, such as your system configuration. \
                     Changes here last until Hotaru quits."
                )
                .size(12.0)
                .role(Role::Text2)
                .wrap()
                .width(Fill),
            ]
            .spacing(8.0)
            .width(Fill)
            .into(),
        );
    }
    page.extend(body);
    let page = aizome_kit::widgets::layout::column(page).spacing(16.0).max_width(PAGE_WIDTH).width(Fill);
    let content = column![
        scroll(container(page).align_x(Align::Center).padding([20.0, 28.0, 32.0, 28.0]).width(Fill))
            .key(("page", app.settings_page))
            .width(Fill)
            .height(Fill),
        status_strip(app),
    ]
    .width(Fill)
    .height(Fill);
    sidebar_layout(aizome_kit::widgets::layout::column(side).height(Fill), content).into()
}

fn status_strip(app: &Hotaru) -> Element<Msg> {
    let mut line = row![].spacing(8.0).align(Align::Center).padding([0.0, 12.0]).height(24.0).width(Fill);
    match &app.settings_notice {
        Some(notice) => {
            let (glyph, role) =
                if notice.error { (Icon::CircleAlert, Role::Danger) } else { (Icon::CircleCheck, Role::Success) };
            line = line
                .push(icon(glyph).size(12.0).role(role))
                .push(text(notice.text.clone()).size(11.0).role(if notice.error { Role::Danger } else { Role::Text2 }));
        }
        None => {
            let path = app.settings_path.as_deref().map_or_else(|| "Not saved: no home folder".into(), short_path);
            line = line.push(text(path).mono().size(10.5).role(Role::Faint));
        }
    }
    container(line.push(spacer())).fill(Tone::Chrome).edges(Edge::Line, Sides::TOP).width(Fill).into()
}

/// `~/…` for a path in the home folder.
fn short_path(path: &std::path::Path) -> String {
    match std::env::var_os("HOME").map(std::path::PathBuf::from) {
        Some(home) if !home.as_os_str().is_empty() && path.starts_with(&home) => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display())
        }
        _ => path.display().to_string(),
    }
}

fn general(app: &Hotaru) -> Vec<Element<Msg>> {
    let s = &app.settings;
    let mut scrollback: Vec<String> = SCROLLBACK.iter().map(|n| lines(*n)).collect();
    let chosen = SCROLLBACK.iter().position(|n| *n == s.scrollback).unwrap_or_else(|| {
        scrollback.push(lines(s.scrollback));
        SCROLLBACK.len()
    });
    let custom = s.scrollback;
    let copy = COPY_FORMATS.iter().position(|(format, _)| *format == s.copy_format);
    let read = ACCESS.iter().position(|(access, _)| *access == s.clipboard_read);
    let write = ACCESS.iter().position(|(access, _)| *access == s.clipboard_write);
    vec![
        form_section(
            "New tabs and splits",
            vec![
                form_row("Shell integration", toggle(s.shell_integration).on_toggle(Msg::SetShellIntegration))
                    .description(
                        "Bash, Zsh and Fish tell Hotaru where they are, so new tabs and splits start in the same \
                         folder. Your dotfiles stay as they are.",
                    )
                    .into(),
                form_row(
                    "Scrollback",
                    dropdown(scrollback, Some(chosen))
                        .on_select(move |i| Msg::SetScrollback(SCROLLBACK.get(i).copied().unwrap_or(custom))),
                )
                .description("What each pane keeps above the screen, the older part compressed. For new panes.")
                .into(),
            ],
        ),
        form_section(
            "Programs",
            vec![
                form_row("Keep ended panes", toggle(s.wait_after_command).on_toggle(Msg::SetWaitAfterCommand))
                    .description(
                        "When the shell or program in a pane ends, its pane closes, then its tab and window with \
                         the last. Keep them to read what they said last; a key closes them.",
                    )
                    .into(),
                form_row("Notifications", toggle(s.desktop_notifications).on_toggle(Msg::SetNotifications))
                    .description(
                        "Programs can tell you when something finishes (OSC 9 and 777) while you're in another \
                         tab or window.",
                    )
                    .into(),
            ],
        ),
        form_section(
            "Clipboard",
            vec![
                form_row(
                    "Copy as",
                    dropdown(COPY_FORMATS.map(|(_, name)| name), copy)
                        .on_select(|i| Msg::SetCopyFormat(COPY_FORMATS[i.min(3)].0)),
                )
                .description("Plain text pastes anywhere; HTML keeps colours in documents and mail.")
                .into(),
                form_row("Copy on select", toggle(s.copy_on_select).on_toggle(Msg::SetCopyOnSelect))
                    .description(
                        "What you select becomes the primary selection, which a middle click pastes, here and in \
                         other programs.",
                    )
                    .into(),
                form_row(
                    "Programs reading it",
                    segmented(ACCESS.map(|(_, name)| name), read)
                        .on_select(|i| Msg::SetClipboardRead(ACCESS[i.min(2)].0)),
                )
                .description(
                    "Editors and tmux, even over SSH, can ask for the clipboard (OSC 52). Hotaru shows you what \
                     they'd get first.",
                )
                .into(),
                form_row(
                    "Programs writing it",
                    segmented(ACCESS.map(|(_, name)| name), write)
                        .on_select(|i| Msg::SetClipboardWrite(ACCESS[i.min(2)].0)),
                )
                .description("They copy for you that way too, into the clipboard or the primary selection.")
                .into(),
            ],
        ),
        form_section(
            "Configuration file",
            vec![
                form_row(
                    "hotaru.toml",
                    row![
                        button("Open").compact().icon(Icon::ExternalLink).on_press(Msg::OpenConfigFile),
                        button("Reload")
                            .compact()
                            .icon(Icon::RefreshCw)
                            .on_press(Msg::Action(Action::ReloadConfig))
                            .tooltip(shortcut_tip(app, "Read the file again", Action::ReloadConfig)),
                    ]
                    .spacing(6.0),
                )
                .description(
                    "Everything here, and what only the file sets: fallback and emoji fonts, OpenType features.",
                )
                .into(),
            ],
        ),
    ]
}

fn lines(n: usize) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("{grouped} lines")
}

fn text_page(app: &Hotaru) -> Vec<Element<Msg>> {
    let s = &app.settings;
    let primary = s.primary_family().to_owned();
    let families: Arc<[String]> = app.monospace_families();
    let chosen = families.iter().position(|family| family.eq_ignore_ascii_case(&primary));
    let names = families.clone();
    let family = dropdown(families.iter().cloned(), chosen)
        .on_select(move |i| Msg::SetFontFamily(names.get(i).cloned().unwrap_or_default()))
        .placeholder(primary.clone())
        .id("font-family");
    let drawn = app.active_pane().and_then(|pane| Some((pane.resolved_font_family()?, pane.font_metrics()?)));
    let family_note = match drawn {
        Some((resolved, _)) if !resolved.eq_ignore_ascii_case(&primary) => {
            format!("“{primary}” isn't installed: drawing with {resolved}.")
        }
        Some((_, metrics)) => {
            format!("Cells are {} × {} pixels at this display's scale.", metrics.cell_width_px, metrics.cell_height_px)
        }
        None if app.monospace.is_none() => "Looking for monospaced fonts…".into(),
        None => "Monospaced fonts installed for you or the system.".into(),
    };
    let weight = WEIGHTS.iter().position(|(w, _)| *w == s.font_weight);
    let zoom = if (app.font_size - s.font_size).abs() > f64::EPSILON {
        format!("This window is zoomed to {} pt for now; ❖0 comes back to this size.", trim(app.font_size))
    } else {
        "❖+ and ❖− zoom the window for now; ❖0 comes back to this size.".into()
    };
    let fallback = s.font_fallback.iter().chain(&s.font_emoji).cloned().collect::<Vec<_>>().join(", ");
    vec![form_section(
        "Font",
        vec![
            form_row("Family", family).description(family_note).into(),
            form_row(
                "Size",
                stepper(s.font_size).range(8.0..=72.0).step(0.5).suffix(" pt").on_change(Msg::SetFontSize),
            )
            .description(zoom)
            .into(),
            form_row(
                "Weight",
                dropdown(WEIGHTS.map(|(_, name)| name), weight)
                    .on_select(|i| Msg::SetWeight(WEIGHTS[i.min(WEIGHTS.len() - 1)].0)),
            )
            .into(),
            form_row("Ligatures", toggle(s.ligatures()).on_toggle(Msg::SetLigatures))
                .description("Join -> != and the like into one glyph, where the font has them.")
                .into(),
            form_row("Fallback", text(fallback).size(12.0).role(Role::Text2).max_lines(2))
                .description("For what the family lacks: Japanese, symbols and emoji.")
                .into(),
        ],
    )]
}

fn trim(points: f64) -> String {
    if points.fract() == 0.0 { format!("{points:.0}") } else { format!("{points:.1}") }
}

fn appearance(app: &Hotaru) -> Vec<Element<Msg>> {
    let s = &app.settings;
    let base = app.desktop_appearance();
    let colors =
        |theme: ThemeChoice| crate::terminal_view::colors_for(&aizome_kit::Theme::new(Appearance { theme, ..base }));
    let night = colors(ThemeChoice::Night);
    let day = colors(ThemeChoice::Day);
    let cards = row![
        theme_card(settings::Theme::Desktop, "Follow Desktop", vec![night.clone(), day.clone()], s.theme),
        theme_card(settings::Theme::Night, "Night", vec![night], s.theme),
        theme_card(settings::Theme::Day, "Day", vec![day], s.theme),
        theme_card(settings::Theme::HighContrast, "High Contrast", vec![colors(ThemeChoice::HighContrast)], s.theme),
    ]
    .spacing(12.0);
    let cursor = CURSORS.iter().position(|(style, _)| *style == s.cursor_style);
    vec![
        form_section(
            "Theme",
            vec![container(cards).padding([14.0, 12.0, 12.0, 12.0]).width(Fill).align_x(Align::Center).into()],
        ),
        form_section(
            "Cursor",
            vec![
                form_row(
                    "Shape",
                    segmented(CURSORS.map(|(_, name)| name), cursor)
                        .on_select(|i| Msg::SetCursorStyle(CURSORS[i.min(2)].0)),
                )
                .into(),
                form_row("Blink", toggle(s.cursor_blink).on_toggle(Msg::SetCursorBlink))
                    .description("Programs may ask for another cursor; they keep it until they reset.")
                    .into(),
            ],
        ),
    ]
}

fn theme_card(
    choice: settings::Theme,
    label: &str,
    colors: Vec<TerminalColors>,
    current: settings::Theme,
) -> Element<Msg> {
    let selected = choice == current;
    let picture = Element::new(
        ThemeCard { colors, selected, msg: MsgFn::new(Msg::SetTheme(choice)), label: label.into() },
        Vec::new(),
    )
    .key(label.to_owned());
    let caption = text(label.to_owned()).size(12.0).weight(if selected { 650.0 } else { 400.0 }).role(if selected {
        Role::Text
    } else {
        Role::Text2
    });
    let caption = mouse_area(caption).on_press(Msg::SetTheme(choice));
    column![picture, caption].spacing(6.0).align(Align::Center).into()
}

fn shortcuts(app: &Hotaru) -> Vec<Element<Msg>> {
    let mut sections: Vec<Element<Msg>> = vec![
        text(
            "Press the pencil to change a command's shortcuts: type a new one, or take one away. ⌥❖ with the \
             arrows, ❖Tab, ❖Space and ⌃❖ with digits stay the desktop's.",
        )
        .size(12.0)
        .role(Role::Text2)
        .wrap()
        .into(),
    ];
    for (group, actions) in Action::groups() {
        let rows = actions
            .iter()
            .map(|(action, label)| form_row(*label, shortcut_editor(app, *action)).key(*label).into())
            .collect();
        sections.push(form_section(group, rows));
    }
    sections.push(
        row![
            text("Changes are kept in hotaru.toml under [keybindings].").size(11.0).role(Role::Text3),
            spacer(),
            button("Restore Defaults")
                .icon(Icon::RotateCcw)
                .enabled(!app.settings.keybindings.is_empty())
                .on_press(Msg::RestoreShortcuts),
        ]
        .width(Fill)
        .into(),
    );
    sections
}

/// A command's shortcuts as keycaps and a pencil; while it's being changed,
/// each one to take away, the field a new one is typed in, and Done.
fn shortcut_editor(app: &Hotaru, action: Action) -> Element<Msg> {
    let shortcuts = app.bindings().shortcuts(action);
    let mut keys: Vec<Element<Msg>> = Vec::new();
    if app.recording == Some(action) {
        for shortcut in shortcuts {
            let label = shortcut.label();
            keys.push(
                button(label.clone())
                    .compact()
                    .mono()
                    .icon(Icon::X)
                    .on_press(Msg::RemoveShortcut(action, shortcut))
                    .tooltip(format!("Take {label} away"))
                    .key(settings::format_shortcut(shortcut))
                    .into(),
            );
        }
        keys.push(recorder(action).id(RECORDER).key("recorder"));
        keys.push(button("Done").compact().on_press(Msg::StopRecording(action)).key("done").into());
    } else {
        if shortcuts.is_empty() {
            keys.push(text("None").size(12.0).role(Role::Faint).into());
        }
        for shortcut in shortcuts {
            keys.push(keycap(shortcut.label()).key(settings::format_shortcut(shortcut)).into());
        }
        keys.push(
            icon_button(Icon::Pencil)
                .compact()
                .on_press(Msg::RecordShortcut(action))
                .tooltip(format!("Change the shortcuts for {}", action.label()))
                .key("change")
                .into(),
        );
    }
    aizome_kit::widgets::layout::row(keys).spacing(6.0).align(Align::Center).into()
}

fn shortcut_tip(app: &Hotaru, tip: &str, action: Action) -> String {
    match app.bindings().shortcut(action) {
        Some(shortcut) => format!("{tip} · {}", shortcut.label()),
        None => tip.to_owned(),
    }
}

fn emit(cx: &mut EventCx, msg: Msg) {
    cx.emit(&MsgFn::new(msg));
}

fn rgb(Rgb(r, g, b): Rgb) -> Color {
    Color::rgb8(r, g, b)
}

/// A theme as a little terminal in its colours: a prompt, a command and
/// what it printed, pressed to choose it. Two themes split corner to
/// corner, as Settei draws Auto.
struct ThemeCard {
    colors: Vec<TerminalColors>,
    selected: bool,
    msg: MsgFn,
    label: Arc<str>,
}

const CARD: Size = Size::new(96.0, 62.0);

impl ThemeCard {
    fn paint_terminal(canvas: &mut aizome_kit::sidestep_ui::Canvas, r: Rect, colors: &TerminalColors) {
        canvas.fill_rect(r, rgb(colors.background));
        let ansi = |i: usize| rgb(colors.palette[i]);
        let fg = rgb(colors.foreground);
        let dim = rgb(colors.palette[8]);
        let x0 = r.x0 + 7.0;
        let line = |row: f64, spans: &[(f64, Color)], canvas: &mut aizome_kit::sidestep_ui::Canvas| {
            let (mut x, y) = (x0, r.y0 + 8.0 + row * 9.0);
            for (width, color) in spans {
                canvas.fill_rect(Rect::new(x, y, (x + width).min(r.x1 - 6.0), y + 3.0), *color);
                x += width + 3.0;
            }
        };
        // A prompt and `cargo test`, what it printed, and the prompt again.
        line(0.0, &[(10.0, ansi(4)), (3.0, ansi(5)), (16.0, fg), (8.0, fg)], canvas);
        line(1.0, &[(20.0, ansi(2)), (26.0, fg)], canvas);
        line(2.0, &[(11.0, ansi(1)), (34.0, dim)], canvas);
        line(3.0, &[(8.0, ansi(3)), (18.0, fg), (14.0, ansi(6))], canvas);
        line(4.0, &[(10.0, ansi(4)), (3.0, ansi(5))], canvas);
        let (x, y) = (x0 + 10.0 + 3.0 + 3.0 + 3.0, r.y0 + 8.0 + 4.0 * 9.0);
        canvas.fill_rect(Rect::new(x, y - 3.0, x + 4.0, y + 6.0), fg);
    }
}

impl Widget for ThemeCard {
    fn name(&self) -> &'static str {
        "theme-card"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(CARD)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let r = cx.bounds();
        match self.colors.as_slice() {
            [one] => Self::paint_terminal(cx.canvas, r, one),
            [first, second, ..] => {
                Self::paint_terminal(cx.canvas, r, first);
                let mut corner = BezPath::new();
                corner.move_to((r.x1, r.y0));
                corner.line_to((r.x1, r.y1));
                corner.line_to((r.x0, r.y1));
                corner.close_path();
                cx.canvas.save();
                cx.canvas.clip(&corner);
                Self::paint_terminal(cx.canvas, r, second);
                cx.canvas.restore();
            }
            [] => {}
        }
        let t = cx.theme();
        let edge = if cx.is_hovered() { t.line2() } else { t.bevel_lo() };
        aizome_kit::draw::border(cx.canvas, r, edge, 1.0);
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        if self.selected {
            aizome_kit::draw::border(cx.canvas, cx.bounds().inflate(2.0, 2.0), t.focus(), 2.0);
        } else if cx.focus_visible() {
            aizome_kit::draw::border(cx.canvas, cx.bounds().inflate(2.0, 2.0), t.focus(), 1.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.selected).str(&self.label);
        for colors in &self.colors {
            h.add(&(colors.foreground, colors.background, &colors.palette[..16]));
        }
    }

    fn hit(&self) -> bool {
        true
    }

    fn overflow(&self) -> f64 {
        3.0
    }

    fn hover_paints(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Tab
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => Status::Handled,
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                if cx.bounds().contains(*pos) {
                    cx.emit(&self.msg.clone());
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        cx.emit(&self.msg.clone());
        true
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::RadioButton,
            label: Some(self.label.to_string()),
            checked: Some(self.selected),
            actions: vec![SemanticAction::Click, SemanticAction::Focus],
            ..Default::default()
        })
    }
}

/// Where a new shortcut is typed: the next key pressed with ❖, ⌃ or ⌥
/// becomes one; Escape, or the focus going elsewhere, gives up.
fn recorder(action: Action) -> Element<Msg> {
    Element::new(Recorder { action }, Vec::new())
}

struct Recorder {
    action: Action,
}

impl Recorder {
    fn spec(&self, t: &Theme) -> TextSpec {
        TextSpec::new(FontSpec::ui(t.metrics.small), t.role(Role::Text3))
    }
}

impl Widget for Recorder {
    fn name(&self) -> &'static str {
        "shortcut-recorder"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let (spec, height) = (self.spec(cx.theme()), cx.theme().metrics.control_compact.max(20.0));
        let width = cx.measure("Type a shortcut", &spec).width.ceil() + 20.0;
        bc.constrain(Size::new(width.max(110.0), height))
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let r = cx.bounds();
        aizome_kit::draw::sunken(cx.canvas, t, r, t.sunk(), t.line2());
        let spec = self.spec(t);
        cx.draw_text_in("Type a shortcut", &spec, r, Align::Center);
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        aizome_kit::draw::focus_ring(cx.canvas, cx.bounds(), t.focus());
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.action);
    }

    fn hit(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Key(key) if key.pressed => {
                let alone = !key.modifiers.super_key() && !key.modifiers.control() && !key.modifiers.alt();
                if alone && !key.modifiers.shift() && key.key == Key::Named(NamedKey::Escape) {
                    emit(cx, Msg::StopRecording(self.action));
                } else if let Some(shortcut) = settings::shortcut_for(key) {
                    emit(cx, Msg::Recorded(self.action, shortcut));
                }
                Status::Handled
            }
            Event::Key(_) | Event::Ime(_) => Status::Handled,
            Event::PointerDown { .. } => {
                cx.request_focus();
                Status::Handled
            }
            Event::Focus(false) => {
                emit(cx, Msg::StopRecording(self.action));
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::TextInput,
            label: Some(format!("New shortcut for {}", self.action.label())),
            ..Default::default()
        })
    }
}

/// A path's `file://` URI, escaped as Hako (and GLib) escape it.
pub fn file_uri(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"!$&'()*+,-./:=@_~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}
