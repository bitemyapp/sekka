//! Menus: the global menu bar an app publishes, context menus and pop-up
//! button menus. A [`MenuBar`] holds [`Menu`]s of [`MenuItem`]s: a label,
//! a message, a [`Shortcut`], an enabled and a checked state, a submenu,
//! and an optional [`MenuRole`] the kit acts on itself (the Edit menu's
//! items go to the focused text widget; the Window menu's to the
//! compositor).

use std::marker::PhantomData;

use sidestep_ui::{Key, KeyEvent, NamedKey};

use crate::element::MsgFn;
use crate::icons::Icon;
use crate::program::AppInfo;
use crate::widget::EditCommand;

/// A key and the modifiers held with it. ❖ (Super) is the command key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub key: ShortcutKey,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// The key of a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutKey {
    /// A key that types this character (letters compare without case).
    Char(char),
    Enter,
    Tab,
    Space,
    Escape,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

impl Shortcut {
    /// ❖ and `key`: `Shortcut::cmd('N')` is ❖N.
    pub fn cmd(key: char) -> Shortcut {
        Shortcut { key: ShortcutKey::Char(key), command: true, shift: false, alt: false, ctrl: false }
    }

    /// A key alone.
    pub fn key(key: ShortcutKey) -> Shortcut {
        Shortcut { key, command: false, shift: false, alt: false, ctrl: false }
    }

    /// ❖ and a key that isn't a character.
    pub fn cmd_key(key: ShortcutKey) -> Shortcut {
        Shortcut { command: true, ..Shortcut::key(key) }
    }

    pub fn shift(mut self) -> Shortcut {
        self.shift = true;
        self
    }

    pub fn alt(mut self) -> Shortcut {
        self.alt = true;
        self
    }

    pub fn ctrl(mut self) -> Shortcut {
        self.ctrl = true;
        self
    }

    /// How menus print it: ⌃ ⌥ ⇧ ❖ then the key.
    pub fn label(&self) -> String {
        use aizome_theme::glyphs::*;
        let mut s = String::new();
        if self.ctrl {
            s.push_str(CONTROL);
        }
        if self.alt {
            s.push_str(ALT);
        }
        if self.shift {
            s.push_str(SHIFT);
        }
        if self.command {
            s.push_str(SUPER);
        }
        s.push_str(&self.key_label());
        s
    }

    pub fn key_label(&self) -> String {
        match self.key {
            ShortcutKey::Char(c) => c.to_uppercase().collect(),
            ShortcutKey::Enter => "↩".into(),
            ShortcutKey::Tab => "⇥".into(),
            ShortcutKey::Space => "Space".into(),
            ShortcutKey::Escape => "⎋".into(),
            ShortcutKey::Backspace => "⌫".into(),
            ShortcutKey::Delete => "⌦".into(),
            ShortcutKey::Up => "↑".into(),
            ShortcutKey::Down => "↓".into(),
            ShortcutKey::Left => "←".into(),
            ShortcutKey::Right => "→".into(),
            ShortcutKey::Home => "↖".into(),
            ShortcutKey::End => "↘".into(),
            ShortcutKey::PageUp => "⇞".into(),
            ShortcutKey::PageDown => "⇟".into(),
            ShortcutKey::F(n) => format!("F{n}"),
        }
    }

    /// Whether a key press is this shortcut.
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        if !ev.pressed {
            return false;
        }
        let m = ev.modifiers;
        if m.super_key() != self.command || m.alt() != self.alt || m.control() != self.ctrl {
            return false;
        }
        match (&ev.key, self.key) {
            (Key::Character(s), ShortcutKey::Char(c)) => {
                let mut chars = s.chars();
                let (Some(k), None) = (chars.next(), chars.next()) else { return false };
                if c.is_alphabetic() {
                    k.to_lowercase().eq(c.to_lowercase()) && m.shift() == self.shift
                } else if k == c {
                    // Punctuation typed with Shift is that punctuation.
                    m.shift() == self.shift || !self.shift
                } else {
                    false
                }
            }
            (Key::Character(s), ShortcutKey::Space) => s == " " && m.shift() == self.shift,
            (Key::Named(n), k) => {
                m.shift() == self.shift
                    && matches!(
                        (n, k),
                        (NamedKey::Enter, ShortcutKey::Enter)
                            | (NamedKey::Tab, ShortcutKey::Tab)
                            | (NamedKey::Escape, ShortcutKey::Escape)
                            | (NamedKey::Backspace, ShortcutKey::Backspace)
                            | (NamedKey::Delete, ShortcutKey::Delete)
                            | (NamedKey::ArrowUp, ShortcutKey::Up)
                            | (NamedKey::ArrowDown, ShortcutKey::Down)
                            | (NamedKey::ArrowLeft, ShortcutKey::Left)
                            | (NamedKey::ArrowRight, ShortcutKey::Right)
                            | (NamedKey::Home, ShortcutKey::Home)
                            | (NamedKey::End, ShortcutKey::End)
                            | (NamedKey::PageUp, ShortcutKey::PageUp)
                            | (NamedKey::PageDown, ShortcutKey::PageDown)
                    )
                    || matches!((n, k), (NamedKey::F(a), ShortcutKey::F(b)) if *a == b)
            }
            _ => false,
        }
    }
}

/// What the kit does with an item itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuRole {
    About,
    /// App-specific preferences: provide a message; a role-only item is disabled.
    Settings,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteAsPlainText,
    Delete,
    SelectAll,
    Find,
    FindNext,
    FindPrevious,
    UseSelectionForFind,
    Replace,
    Minimise,
    Fill,
    StackWith,
    BringAllToFront,
    CloseWindow,
    /// The Window menu's list of windows (the kit fills it).
    WindowList,
    /// Help ▸ Search: the shell's palette, searching the app's commands.
    HelpSearch,
    /// App-specific help: provide a message; a role-only item is disabled.
    Help,
    /// Help ▸ Aizome Tour: Annai's welcome and tour of the desktop.
    Tour,
}

impl MenuRole {
    /// The Edit command the item sends to the focused widget.
    pub fn edit_command(self) -> Option<EditCommand> {
        Some(match self {
            MenuRole::Undo => EditCommand::Undo,
            MenuRole::Redo => EditCommand::Redo,
            MenuRole::Cut => EditCommand::Cut,
            MenuRole::Copy => EditCommand::Copy,
            MenuRole::Paste => EditCommand::Paste,
            MenuRole::PasteAsPlainText => EditCommand::PasteAsPlainText,
            MenuRole::Delete => EditCommand::Delete,
            MenuRole::SelectAll => EditCommand::SelectAll,
            MenuRole::Find => EditCommand::Find,
            MenuRole::FindNext => EditCommand::FindNext,
            MenuRole::FindPrevious => EditCommand::FindPrevious,
            MenuRole::UseSelectionForFind => EditCommand::UseSelectionForFind,
            MenuRole::Replace => EditCommand::Replace,
            _ => return None,
        })
    }

    /// Its name in the shell protocol.
    pub fn name(self) -> &'static str {
        match self {
            MenuRole::About => "about",
            MenuRole::Settings => "settings",
            MenuRole::Services => "services",
            MenuRole::Hide => "hide",
            MenuRole::HideOthers => "hide-others",
            MenuRole::ShowAll => "show-all",
            MenuRole::Quit => "quit",
            MenuRole::Undo => "undo",
            MenuRole::Redo => "redo",
            MenuRole::Cut => "cut",
            MenuRole::Copy => "copy",
            MenuRole::Paste => "paste",
            MenuRole::PasteAsPlainText => "paste-as-plain-text",
            MenuRole::Delete => "delete",
            MenuRole::SelectAll => "select-all",
            MenuRole::Find => "find",
            MenuRole::FindNext => "find-next",
            MenuRole::FindPrevious => "find-previous",
            MenuRole::UseSelectionForFind => "use-selection-for-find",
            MenuRole::Replace => "replace",
            MenuRole::Minimise => "minimise",
            MenuRole::Fill => "fill",
            MenuRole::StackWith => "stack-with",
            MenuRole::BringAllToFront => "bring-all-to-front",
            MenuRole::CloseWindow => "close-window",
            MenuRole::WindowList => "window-list",
            MenuRole::HelpSearch => "help-search",
            MenuRole::Help => "help",
            MenuRole::Tour => "tour",
        }
    }
}

/// A menu without its message type: what the kit keeps and draws.
#[derive(Clone, Debug, Default)]
pub struct MenuSpec {
    pub title: String,
    pub items: Vec<ItemSpec>,
    /// Its glyph where it's a submenu's row (the Toolchest's).
    pub icon: Option<Icon>,
}

#[derive(Clone, Debug)]
pub enum ItemSpec {
    Action(ActionSpec),
    Separator,
    Submenu(MenuSpec),
    /// Text that does nothing (a section's name).
    Header(String),
}

#[derive(Clone, Debug)]
pub struct ActionSpec {
    pub label: String,
    pub msg: Option<MsgFn>,
    pub shortcut: Option<Shortcut>,
    pub enabled: bool,
    pub checked: bool,
    pub role: Option<MenuRole>,
    /// A stable id for the shell protocol, if the app picks it.
    pub id: Option<u32>,
    pub icon: Option<Icon>,
}

impl MenuSpec {
    /// The enabled item `ev` is the shortcut of, here or in a submenu.
    pub fn find_shortcut(&self, ev: &KeyEvent) -> Option<&ActionSpec> {
        self.items.iter().find_map(|i| match i {
            ItemSpec::Action(a) if a.enabled && a.shortcut.is_some_and(|s| s.matches(ev)) => Some(a),
            ItemSpec::Submenu(m) => m.find_shortcut(ev),
            _ => None,
        })
    }

    /// Every action, depth first.
    pub fn actions(&self) -> Vec<&ActionSpec> {
        let mut out = Vec::new();
        for i in &self.items {
            match i {
                ItemSpec::Action(a) => out.push(a),
                ItemSpec::Submenu(m) => out.extend(m.actions()),
                _ => {}
            }
        }
        out
    }

    /// The items' actions with their roles' states settled by `state`.
    pub(crate) fn settle(&mut self, state: &dyn Fn(&ActionSpec) -> Option<bool>) {
        for i in &mut self.items {
            match i {
                ItemSpec::Action(a) => {
                    if let Some(on) = state(a) {
                        a.enabled = on;
                    }
                }
                ItemSpec::Submenu(m) => m.settle(state),
                _ => {}
            }
        }
    }
}

/// A menu: `Menu::new("File").item(MenuItem::new("New", Msg::New).shortcut(Shortcut::cmd('N')))`.
pub struct Menu<Msg> {
    pub(crate) spec: MenuSpec,
    _m: PhantomData<fn() -> Msg>,
}

impl<Msg> Clone for Menu<Msg> {
    fn clone(&self) -> Self {
        Menu { spec: self.spec.clone(), _m: PhantomData }
    }
}

impl<Msg> Menu<Msg> {
    pub fn new(title: impl Into<String>) -> Menu<Msg> {
        Menu { spec: MenuSpec { title: title.into(), items: Vec::new(), icon: None }, _m: PhantomData }
    }

    pub fn item(mut self, item: MenuItem<Msg>) -> Self {
        self.spec.items.push(item.spec);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = MenuItem<Msg>>) -> Self {
        self.spec.items.extend(items.into_iter().map(|i| i.spec));
        self
    }

    pub fn separator(mut self) -> Self {
        self.spec.items.push(ItemSpec::Separator);
        self
    }

    /// A section's name, as text that does nothing.
    pub fn header(mut self, text: impl Into<String>) -> Self {
        self.spec.items.push(ItemSpec::Header(text.into()));
        self
    }

    pub fn submenu(mut self, menu: Menu<Msg>) -> Self {
        self.spec.items.push(ItemSpec::Submenu(menu.spec));
        self
    }

    pub fn push(&mut self, item: MenuItem<Msg>) {
        self.spec.items.push(item.spec);
    }

    pub fn title(&self) -> &str {
        &self.spec.title
    }

    pub fn spec(&self) -> &MenuSpec {
        &self.spec
    }

    pub fn len(&self) -> usize {
        self.spec.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spec.items.is_empty()
    }
}

/// An item of a menu.
pub struct MenuItem<Msg> {
    pub(crate) spec: ItemSpec,
    _m: PhantomData<fn() -> Msg>,
}

impl<Msg: Clone + Send + 'static> MenuItem<Msg> {
    /// An item that sends `msg`.
    pub fn new(label: impl Into<String>, msg: Msg) -> MenuItem<Msg> {
        MenuItem::make(label.into(), Some(MsgFn::new(msg)))
    }
}

impl<Msg> MenuItem<Msg> {
    fn make(label: String, msg: Option<MsgFn>) -> MenuItem<Msg> {
        MenuItem {
            spec: ItemSpec::Action(ActionSpec {
                label,
                msg,
                shortcut: None,
                enabled: true,
                checked: false,
                role: None,
                id: None,
                icon: None,
            }),
            _m: PhantomData,
        }
    }

    /// An item the kit carries out itself, by its role.
    pub fn role_item(label: impl Into<String>, role: MenuRole) -> MenuItem<Msg> {
        MenuItem::make(label.into(), None).role(role)
    }

    pub fn separator() -> MenuItem<Msg> {
        MenuItem { spec: ItemSpec::Separator, _m: PhantomData }
    }

    pub fn submenu(menu: Menu<Msg>) -> MenuItem<Msg> {
        MenuItem { spec: ItemSpec::Submenu(menu.spec), _m: PhantomData }
    }

    fn action(&mut self) -> Option<&mut ActionSpec> {
        match &mut self.spec {
            ItemSpec::Action(a) => Some(a),
            _ => None,
        }
    }

    pub fn shortcut(mut self, s: Shortcut) -> Self {
        if let Some(a) = self.action() {
            a.shortcut = Some(s);
        }
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        if let Some(a) = self.action() {
            a.enabled = on;
        }
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        if let Some(a) = self.action() {
            a.checked = on;
        }
        self
    }

    pub fn role(mut self, role: MenuRole) -> Self {
        if let Some(a) = self.action() {
            a.role = Some(role);
        }
        self
    }

    /// Its id in the shell protocol (else one is derived from its place
    /// and label).
    pub fn id(mut self, id: u32) -> Self {
        if let Some(a) = self.action() {
            a.id = Some(id);
        }
        self
    }

    /// A glyph before its label: an action's, or a submenu's.
    pub fn icon(mut self, icon: Icon) -> Self {
        match &mut self.spec {
            ItemSpec::Action(a) => a.icon = Some(icon),
            ItemSpec::Submenu(m) => m.icon = Some(icon),
            _ => {}
        }
        self
    }
}

/// The menus an app shows in the global menu bar (or along its windows'
/// tops where there's no Aizome shell).
pub struct MenuBar<Msg> {
    pub(crate) menus: Vec<MenuSpec>,
    _m: PhantomData<fn() -> Msg>,
}

impl<Msg> Clone for MenuBar<Msg> {
    fn clone(&self) -> Self {
        MenuBar { menus: self.menus.clone(), _m: PhantomData }
    }
}

impl<Msg> Default for MenuBar<Msg> {
    fn default() -> Self {
        MenuBar { menus: Vec::new(), _m: PhantomData }
    }
}

/// The titles of the standard menus.
pub const EDIT: &str = "Edit";
pub const WINDOW: &str = "Window";
pub const HELP: &str = "Help";

impl<Msg> MenuBar<Msg> {
    pub fn new() -> MenuBar<Msg> {
        MenuBar::default()
    }

    /// Use Control for command shortcuts when embedding an app in a desktop
    /// that reserves Super. This updates both matching and the visible labels,
    /// including nested menus. Explicit Control+Super combinations stay intact.
    pub fn control_commands(mut self) -> Self {
        fn convert(menu: &mut MenuSpec) {
            for item in &mut menu.items {
                match item {
                    ItemSpec::Action(action) => {
                        if let Some(shortcut) = &mut action.shortcut
                            && shortcut.command
                            && !shortcut.ctrl
                        {
                            shortcut.command = false;
                            shortcut.ctrl = true;
                        }
                    }
                    ItemSpec::Submenu(menu) => convert(menu),
                    _ => {}
                }
            }
        }
        for menu in &mut self.menus {
            convert(menu);
        }
        self
    }

    /// The app menu, Edit, Window and Help, as every Aizome app has them.
    /// Apps with settings add their own Settings action to the app menu;
    /// there is no default settings dialog for this menu to open.
    pub fn standard(info: &AppInfo) -> MenuBar<Msg> {
        let name = &info.name;
        let i = |label: &str, role: MenuRole| MenuItem::<Msg>::role_item(label, role);
        let app = Menu::new(name.clone())
            .item(i(&format!("About {name}"), MenuRole::About))
            .separator()
            .submenu(Menu::new("Services").item(i("No Services Apply", MenuRole::Services).enabled(false)))
            .separator()
            .item(i(&format!("Hide {name}"), MenuRole::Hide).shortcut(Shortcut::cmd('H')))
            .item(i("Hide Others", MenuRole::HideOthers).shortcut(Shortcut::cmd('H').alt()))
            .separator()
            .item(i(&format!("Quit {name}"), MenuRole::Quit).shortcut(Shortcut::cmd('Q')));
        let find = Menu::new("Find")
            .item(i("Find…", MenuRole::Find).shortcut(Shortcut::cmd('F')))
            .item(i("Find Next", MenuRole::FindNext).shortcut(Shortcut::cmd('G')))
            .item(i("Find Previous", MenuRole::FindPrevious).shortcut(Shortcut::cmd('G').shift()))
            .item(i("Use Selection for Find", MenuRole::UseSelectionForFind).shortcut(Shortcut::cmd('E')))
            .item(i("Replace…", MenuRole::Replace).shortcut(Shortcut::cmd('F').alt()));
        let edit = Menu::new(EDIT)
            .item(i("Undo", MenuRole::Undo).shortcut(Shortcut::cmd('Z')))
            .item(i("Redo", MenuRole::Redo).shortcut(Shortcut::cmd('Z').shift()))
            .separator()
            .item(i("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
            .item(i("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('C')))
            .item(i("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
            .item(i("Paste as Plain Text", MenuRole::PasteAsPlainText).shortcut(Shortcut::cmd('V').shift().alt()))
            .item(i("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
            .separator()
            .submenu(find);
        let window = Menu::new(WINDOW)
            .item(i("Minimise", MenuRole::Minimise).shortcut(Shortcut::cmd('M')))
            .item(i("Fill", MenuRole::Fill))
            .item(i("Stack with…", MenuRole::StackWith))
            .separator()
            .item(i("Bring All to Front", MenuRole::BringAllToFront))
            .separator()
            .item(i("Windows", MenuRole::WindowList).enabled(false));
        let help = Menu::new(HELP)
            .item(i("Search", MenuRole::HelpSearch))
            .item(i(&format!("{name} Help"), MenuRole::Help))
            .separator()
            .item(i("Aizome Tour", MenuRole::Tour));
        MenuBar { menus: vec![app.spec, edit.spec, window.spec, help.spec], _m: PhantomData }
    }

    /// Add `menu` before Window and Help (an app's View, Format, …).
    pub fn with_menu(mut self, menu: Menu<Msg>) -> Self {
        let at = self.menus.iter().position(|m| m.title == WINDOW || m.title == HELP).unwrap_or(self.menus.len());
        self.menus.insert(at, menu.spec);
        self
    }

    /// Add `menu` right after the app menu (File).
    pub fn with_file_menu(mut self, menu: Menu<Msg>) -> Self {
        let at = 1.min(self.menus.len());
        self.menus.insert(at, menu.spec);
        self
    }

    /// Put `menu` at `index`.
    pub fn insert(mut self, index: usize, menu: Menu<Msg>) -> Self {
        let at = index.min(self.menus.len());
        self.menus.insert(at, menu.spec);
        self
    }

    /// Replace the menu titled `title` (or add it before Window).
    pub fn replace(mut self, title: &str, menu: Menu<Msg>) -> Self {
        match self.menus.iter().position(|m| m.title == title) {
            Some(i) => {
                self.menus[i] = menu.spec;
                self
            }
            None => self.with_menu(menu),
        }
    }

    pub fn remove(mut self, title: &str) -> Self {
        self.menus.retain(|m| m.title != title);
        self
    }

    /// Add items at the end of the menu titled `title`.
    pub fn extend(mut self, title: &str, items: impl IntoIterator<Item = MenuItem<Msg>>) -> Self {
        if let Some(m) = self.menus.iter_mut().find(|m| m.title == title) {
            m.items.extend(items.into_iter().map(|i| i.spec));
        }
        self
    }

    pub fn menus(&self) -> &[MenuSpec] {
        &self.menus
    }

    pub(crate) fn into_specs(self) -> Vec<MenuSpec> {
        self.menus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidestep_ui::Modifiers;

    fn press(key: Key, mods: Modifiers) -> KeyEvent {
        KeyEvent { key, text: None, pressed: true, repeat: false, code: 0, modifiers: mods, composing: None }
    }

    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }

    #[test]
    fn control_commands_match_and_label_nested_menus() {
        let info = AppInfo::new("test", "Test", "Test");
        let standard = MenuBar::<()>::standard(&info);
        let external = standard.clone().control_commands();
        let find = |bar: &MenuBar<()>, key, modifiers| {
            bar.menus().iter().find_map(|menu| menu.find_shortcut(&press(ch(key), modifiers)).cloned())
        };
        assert!(find(&standard, "c", Modifiers::SUPER).is_some());
        assert!(find(&standard, "c", Modifiers::CONTROL).is_none());
        for key in ["c", "v", "x", "a", "f", "q"] {
            let action = find(&external, key, Modifiers::CONTROL).expect(key);
            let shortcut = action.shortcut.unwrap();
            assert!(shortcut.ctrl && !shortcut.command);
            assert!(!shortcut.label().contains(aizome_theme::glyphs::SUPER));
            assert!(find(&external, key, Modifiers::SUPER).is_none());
        }
        assert!(find(&external, "g", Modifiers::CONTROL | Modifiers::SHIFT).is_some());
    }

    #[test]
    fn shortcuts_match_keys_and_modifiers() {
        let copy = Shortcut::cmd('C');
        assert!(copy.matches(&press(ch("c"), Modifiers::SUPER)));
        assert!(!copy.matches(&press(ch("c"), Modifiers::SUPER | Modifiers::SHIFT)));
        assert!(!copy.matches(&press(ch("c"), Modifiers::CONTROL)));
        let redo = Shortcut::cmd('Z').shift();
        assert!(redo.matches(&press(ch("Z"), Modifiers::SUPER | Modifiers::SHIFT)));
        assert!(!redo.matches(&press(ch("z"), Modifiers::SUPER)));
        let plain = Shortcut::cmd('V').shift().alt();
        assert!(plain.matches(&press(ch("V"), Modifiers::SUPER | Modifiers::SHIFT | Modifiers::ALT)));
        // Punctuation typed with Shift.
        let help = Shortcut::cmd('?');
        assert!(help.matches(&press(ch("?"), Modifiers::SUPER | Modifiers::SHIFT)));
        assert!(Shortcut::cmd(',').matches(&press(ch(","), Modifiers::SUPER)));
        let up = Shortcut::cmd_key(ShortcutKey::Up);
        assert!(up.matches(&press(Key::Named(NamedKey::ArrowUp), Modifiers::SUPER)));
        assert!(!up.matches(&press(Key::Named(NamedKey::ArrowDown), Modifiers::SUPER)));
    }

    #[test]
    fn labels_print_modifiers_in_aizome_order() {
        assert_eq!(Shortcut::cmd('V').shift().alt().label(), "⌥⇧❖V");
        assert_eq!(Shortcut::cmd('z').shift().label(), "⇧❖Z");
        assert_eq!(Shortcut::cmd(',').label(), "❖,");
        assert_eq!(Shortcut::cmd('q').ctrl().label(), "⌃❖Q");
    }

    #[test]
    fn the_standard_bar_has_the_standard_menus() {
        let info = AppInfo::new("org.aizome.Soroban", "Soroban", "Calculator");
        let bar: MenuBar<()> = MenuBar::standard(&info);
        let titles: Vec<&str> = bar.menus().iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["Soroban", "Edit", "Window", "Help"]);
        let quit = bar.menus()[0].find_shortcut(&press(ch("q"), Modifiers::SUPER)).expect("❖Q");
        assert_eq!((quit.label.as_str(), quit.role), ("Quit Soroban", Some(MenuRole::Quit)));
        let find = bar.menus()[1].find_shortcut(&press(ch("f"), Modifiers::SUPER)).expect("❖F in Find ▸");
        assert_eq!(find.role, Some(MenuRole::Find));
        let bar = bar.with_file_menu(Menu::new("File")).with_menu(Menu::new("Format"));
        let titles: Vec<&str> = bar.menus().iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["Soroban", "File", "Edit", "Format", "Window", "Help"]);
    }

    #[test]
    fn standard_menus_do_not_offer_unimplemented_settings() {
        let info = AppInfo::new("org.aizome.Soroban", "Soroban", "Calculator");
        let bar: MenuBar<()> = MenuBar::standard(&info);
        assert!(bar.menus()[0].find_shortcut(&press(ch(","), Modifiers::SUPER)).is_none());
        assert!(
            !bar.menus()[0]
                .items
                .iter()
                .any(|item| { matches!(item, ItemSpec::Action(a) if a.role == Some(MenuRole::Settings)) })
        );
    }
}
