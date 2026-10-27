//! General: a list of pages, as macOS's General is. About (the mark, the
//! machine's name and hardware), Software Update (Yukimi's Updates),
//! Storage (what fills the disk, Kura), Startup (the boot splash, through
//! Yukimi), Date & Time, Language & Region, Default Apps and Login Items.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::form;
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::widgets::{PressStyle, press, tile};

mod about;
mod datetime;
pub use datetime::clock_text;
mod defaults;
mod language;
mod login;
mod startup;
mod storage;
mod update;

/// A page of General.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    #[default]
    Main,
    About,
    SoftwareUpdate,
    Storage,
    Startup,
    DateTime,
    Language,
    DefaultApps,
    LoginItems,
}

impl Page {
    pub const PAGES: [Page; 8] = [
        Page::About,
        Page::SoftwareUpdate,
        Page::Storage,
        Page::Startup,
        Page::DateTime,
        Page::Language,
        Page::DefaultApps,
        Page::LoginItems,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Main => "General",
            Page::About => "About",
            Page::SoftwareUpdate => "Software Update",
            Page::Storage => "Storage",
            Page::Startup => "Startup",
            Page::DateTime => "Date & Time",
            Page::Language => "Language & Region",
            Page::DefaultApps => "Default Apps",
            Page::LoginItems => "Login Items",
        }
    }

    /// The page's own entry id (what `--pane general.about` names).
    pub fn entry(self) -> &'static str {
        match self {
            Page::Main => "",
            Page::About => "about",
            Page::SoftwareUpdate => "software-update",
            Page::Storage => "storage",
            Page::Startup => "startup",
            Page::DateTime => "datetime",
            Page::Language => "language",
            Page::DefaultApps => "default-apps",
            Page::LoginItems => "login-items",
        }
    }

    fn icon(self) -> (Icon, Tile) {
        match self {
            Page::Main => (Icon::Settings, Tile::Slate),
            Page::About => (Icon::Info, Tile::Ai),
            Page::SoftwareUpdate => (Icon::Snowflake, Tile::Hanada),
            Page::Storage => (Icon::HardDrive, Tile::Slate),
            Page::Startup => (Icon::Power, Tile::Kon),
            Page::DateTime => (Icon::Clock, Tile::Kohaku),
            Page::Language => (Icon::Languages, Tile::Cyan),
            Page::DefaultApps => (Icon::AppWindow, Tile::Violet),
            Page::LoginItems => (Icon::ListChecks, Tile::Green),
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Page::About => "This computer, its name and what it's made of.",
            Page::SoftwareUpdate => "Keep the system and your apps current, with Yukimi.",
            Page::Storage => "What fills the disk, and what could go.",
            Page::Startup => "What the computer shows while it starts and stops.",
            Page::DateTime => "The time zone, network time and the clock.",
            Page::Language => "The language and how dates, times and numbers look.",
            Page::DefaultApps => "Which app opens links, mail, pictures and the rest.",
            Page::LoginItems => "What starts when you log in.",
            Page::Main => "",
        }
    }
}

#[derive(Default)]
pub struct General {
    page: Page,
    about: about::State,
    update: update::State,
    storage: storage::State,
    startup: startup::State,
    time: datetime::State,
    language: language::State,
    defaults: defaults::State,
    login: login::State,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Page(Page),
    About(about::Msg),
    Update(update::Msg),
    Storage(storage::Msg),
    Startup(startup::Msg),
    Time(datetime::Msg),
    Language(language::Msg),
    Defaults(defaults::Msg),
    Login(login::Msg),
}

impl General {
    fn page_shown(&mut self, cx: &mut PaneCx<Msg>) {
        match self.page {
            Page::Main => {}
            Page::About => self.about.shown(cx),
            Page::SoftwareUpdate => self.update.shown(cx),
            Page::Storage => self.storage.shown(cx),
            Page::Startup => self.startup.shown(cx),
            Page::DateTime => self.time.shown(cx),
            Page::Language => self.language.shown(cx),
            Page::DefaultApps => self.defaults.shown(cx),
            Page::LoginItems => self.login.shown(cx),
        }
    }

    fn page_hidden(&mut self, cx: &mut PaneCx<Msg>) {
        if self.page == Page::DateTime {
            self.time.hidden(cx);
        }
    }

    fn go(&mut self, page: Page, cx: &mut PaneCx<Msg>) {
        if page == self.page {
            return;
        }
        self.page_hidden(cx);
        self.page = page;
        self.page_shown(cx);
    }

    fn main_view(&self, vcx: &ViewCx) -> Element<Msg> {
        let summary = about::summary(&self.about, vcx);
        let rows: Vec<form::Row<Msg>> = Page::PAGES
            .iter()
            .map(|p| {
                let (glyph, t) = p.icon();
                let line = row![
                    tile(glyph, t, 24.0),
                    column![text(p.title()), text(p.blurb()).size(11.0).role(Role::Text3)].spacing(2.0).width(Fill),
                    icon(Icon::ChevronRight).size(14.0).role(Role::Text3),
                ]
                .spacing(12.0)
                .align(Align::Center)
                .width(Fill);
                form::whole(
                    press(container(line).padding([8.0, form::INSET]).width(Fill))
                        .style(PressStyle::Plain)
                        .on_press(Msg::Page(*p))
                        .named(p.title())
                        .width(Fill),
                )
                .entry(p.entry())
            })
            .collect();
        form::page(vec![summary, vcx.section("", rows)])
    }
}

impl Pane for General {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "general"
    }

    fn title(&self) -> &'static str {
        "General"
    }

    fn icon(&self) -> Icon {
        Icon::Settings
    }

    fn tile(&self) -> Tile {
        Tile::Slate
    }

    fn group(&self) -> Group {
        Group::System
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["system", "computer"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        let mut v = vec![
            SettingEntry::new("about", "About")
                .keywords(&["computer", "hardware", "version", "sekka", "nixos", "specs"]),
            SettingEntry::new("software-update", "Software Update").keywords(&["upgrade", "yukimi", "nixos-rebuild"]),
            SettingEntry::new("storage", "Storage").keywords(&["disk space", "free space", "kura", "full"]),
            SettingEntry::new("startup", "Startup").keywords(&["boot", "start up", "splash", "shut down"]),
            SettingEntry::new("datetime", "Date & Time").keywords(&["clock", "time zone", "timezone"]),
            SettingEntry::new("language", "Language & Region").keywords(&[
                "locale",
                "formats",
                "region",
                "translation",
            ]),
            SettingEntry::new("default-apps", "Default Apps").keywords(&["open with", "browser", "mail", "handlers"]),
            SettingEntry::new("login-items", "Login Items").keywords(&["autostart", "startup", "launch at login"]),
        ];
        v.extend(about::entries());
        v.extend(update::entries());
        v.extend(storage::entries());
        v.extend(startup::entries());
        v.extend(datetime::entries());
        v.extend(language::entries());
        v.extend(defaults::entries());
        v.extend(login::entries());
        v
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        // The summary on the main page wants the machine's name.
        self.about.shown(cx);
        self.page_shown(cx);
    }

    fn hidden(&mut self, cx: &mut PaneCx<Msg>) {
        self.page_hidden(cx);
    }

    fn system_changed(&mut self, cx: &mut PaneCx<Msg>) {
        // The locale changed (here, or elsewhere): the preview follows.
        if self.page == Page::Language {
            self.language.system_changed(cx);
        }
    }

    fn focus_entry(&mut self, entry: &str, cx: &mut PaneCx<Msg>) {
        let under = self.entries().into_iter().find(|e| e.id == entry).and_then(|e| e.under.first().cloned());
        let page = match under {
            Some(title) => Page::PAGES.iter().copied().find(|p| p.title() == title),
            None => Page::PAGES.iter().copied().find(|p| p.entry() == entry).map(|_| Page::Main),
        };
        // A page's own entry opens the page.
        let page = Page::PAGES.iter().copied().find(|p| p.entry() == entry).or(page).unwrap_or(Page::Main);
        self.go(page, cx);
    }

    fn crumb(&self) -> Option<&'static str> {
        (self.page != Page::Main).then(|| self.page.title())
    }

    fn back(&mut self, cx: &mut PaneCx<Msg>) -> bool {
        if self.page == Page::Main {
            return false;
        }
        self.go(Page::Main, cx);
        true
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Page(p) => self.go(p, cx),
            Msg::About(m) => self.about.update(m, cx),
            Msg::Update(m) => self.update.update(m, cx),
            Msg::Storage(m) => self.storage.update(m, cx),
            Msg::Startup(m) => self.startup.update(m, cx),
            Msg::Time(m) => self.time.update(m, cx),
            Msg::Language(m) => self.language.update(m, cx),
            Msg::Defaults(m) => self.defaults.update(m, cx),
            Msg::Login(m) => self.login.update(m, cx),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        if self.page == Page::Main {
            return self.main_view(vcx);
        }
        let (glyph, t) = self.page.icon();
        let head = vcx.anchor(
            self.page.entry(),
            row![tile(glyph, t, 28.0), text(self.page.title()).size(20.0).weight(650.0)]
                .spacing(10.0)
                .align(Align::Center)
                .width(Fill),
        );
        let body: Element<Msg> = match self.page {
            Page::Main => unreachable!(),
            Page::About => self.about.view(vcx).map(Msg::About),
            Page::SoftwareUpdate => self.update.view(vcx).map(Msg::Update),
            Page::Storage => self.storage.view(vcx).map(Msg::Storage),
            Page::Startup => self.startup.view(vcx).map(Msg::Startup),
            Page::DateTime => self.time.view(vcx).map(Msg::Time),
            Page::Language => self.language.view(vcx).map(Msg::Language),
            Page::DefaultApps => self.defaults.view(vcx).map(Msg::Defaults),
            Page::LoginItems => self.login.view(vcx).map(Msg::Login),
        };
        column![head, body].spacing(16.0).width(Fill).into()
    }
}
