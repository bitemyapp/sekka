//! The window: a sidebar of places, a toolbar, the page in view, and a
//! status line; and the sheets (a package's details, choosing a branch,
//! an operation's progress).
//!
//! Views are drawn from the program's state every time it changes, so each
//! is a plain function of it. The frame is the kit's (`sidebar_layout`,
//! `sidebar_item`, `toolbar`, `scroll`, `status_bar`, `search_field`);
//! what Yukimi draws of its own (dye bars, the generations' chart,
//! monograms, the snow) is in [`widgets`].
use aizome_kit::prelude::*;
use aizome_kit::text::Text;
use aizome_kit::widgets::layout::Container;
use aizome_kit::{column, row};

use crate::app::{Msg, Page, Yukimi};

mod comparison;
mod desktops;
mod discover;
mod history;
mod installed;
mod job;
mod overview;
mod storage;
mod updates;
pub mod widgets;

use widgets::{Dye, chip};

/// The sidebar's width.
pub const SIDEBAR: f64 = 212.0;
/// The widest a page's content gets.
pub const PAGE_WIDTH: f64 = 920.0;

pub fn view(app: &Yukimi, window: WindowKey) -> Element<Msg> {
    if let Some(comparison) = &app.comparison
        && comparison.sheet == window
    {
        return comparison::sheet(comparison);
    }
    if app.job_sheet == Some(window) {
        return job::sheet(app);
    }
    if let Some((sheet, package)) = &app.details
        && *sheet == window
    {
        return discover::details(app, package, *sheet);
    }
    if let Some(chooser) = &app.branch
        && chooser.sheet == window
    {
        return updates::branch_sheet(app, chooser);
    }
    let page: Element<Msg> = match app.page {
        Page::Overview => overview::page(app),
        Page::Installed => installed::page(app),
        Page::Discover => discover::page(app),
        Page::Updates => updates::page(app),
        Page::History => history::page(app),
        Page::Storage => storage::page(app),
        Page::Desktops => desktops::page(app),
    };
    let content = container(page.max_width(PAGE_WIDTH).width(Fill))
        .align_x(Align::Center)
        .width(Fill)
        .padding([20.0, 24.0, 28.0, 24.0]);
    let main = column![tool_bar(app), scroll(content).key(app.page).width(Fill).height(Fill), status_line(app)]
        .width(Fill)
        .height(Fill);
    sidebar_layout(sidebar(app), main).first_size(SIDEBAR).min_sizes(180.0, 520.0).into()
}

pub fn menus(app: &Yukimi) -> MenuBar<Msg> {
    let go = Page::ALL.iter().enumerate().filter(|(_, p)| offers(app, **p)).fold(Menu::new("Go"), |menu, (i, page)| {
        menu.item(
            MenuItem::new(page.title(), Msg::Show(*page))
                .shortcut(Shortcut::cmd(char::from_digit(i as u32 + 1, 10).unwrap_or('1')))
                .checked(app.page == *page)
                .icon(page.icon()),
        )
    });
    let system = Menu::new("System")
        .item(MenuItem::new("Read Everything Again", Msg::Reload).shortcut(Shortcut::cmd('R')))
        .item(MenuItem::new("Check for Updates", Msg::CheckUpdates).shortcut(Shortcut::cmd('U')).enabled(!app.checking))
        .separator()
        .item(MenuItem::new("Search Packages", Msg::Show(Page::Discover)).shortcut(Shortcut::cmd('F').shift()))
        .item(MenuItem::new("Clean Up the Store…", Msg::CleanUp).enabled(app.model.store.is_some()));
    MenuBar::standard(&app.info()).with_menu(go).with_menu(system)
}

// The frame.

/// Whether the sidebar offers `page`: Desktops only on a system that
/// declares choices Yukimi may change (or while it is shown).
pub fn offers(app: &Yukimi, page: Page) -> bool {
    page != Page::Desktops || !app.choices().is_empty() || app.page == page
}

/// How many packages are installed, for the sidebar and the Overview:
/// (chosen for everyone, just for you).
pub fn package_counts(app: &Yukimi) -> (usize, usize) {
    let configured = &app.model.configured;
    let chosen = configured.yukimi.packages.len()
        + configured.yukimi.programs.len()
        + configured.system.len()
        + configured.settings.values().map(Vec::len).sum::<usize>();
    let mine = app.model.user_packages.len() + configured.user.len();
    (chosen, mine)
}

/// A place in the sidebar, with a count or a state at its right.
fn place(app: &Yukimi, page: Page) -> Element<Msg> {
    let detail: Option<String> = match page {
        Page::Installed => {
            let (chosen, mine) = package_counts(app);
            let rest = app.model.store.as_ref().map_or(0, |s| s.system_packages.len().saturating_sub(chosen));
            (!app.loading).then(|| (chosen + mine + rest).to_string())
        }
        Page::Updates => {
            let newer = app.movements().map_or(0, |m| crate::checks::newer_count(&m));
            if app.checking {
                Some("…".to_owned())
            } else if let Some(word) = match app.model.completion.action {
                yukimi_system::completion::Action::Reboot => Some("restart"),
                yukimi_system::completion::Action::Logout => Some("sign out"),
                yukimi_system::completion::Action::Reload => Some("reload"),
                yukimi_system::completion::Action::Unknown => Some("see why"),
                yukimi_system::completion::Action::None => None,
            } {
                Some(word.to_owned())
            } else {
                (newer > 0).then(|| format!("{newer} new"))
            }
        }
        Page::History => (!app.model.generations.is_empty()).then(|| app.model.generations.len().to_string()),
        Page::Storage => app.model.store.as_ref().map(|s| short_size(s.composition.total)),
        Page::Desktops => (!app.choice_changes().is_empty()).then(|| "changed".to_owned()),
        _ => None,
    };
    let mut item = sidebar_item(page.icon(), page.title(), app.page == page).on_press(Msg::Show(page));
    if let Some(d) = detail {
        item = item.detail(d);
    }
    item.into()
}

fn sidebar(app: &Yukimi) -> Element<Msg> {
    let brand = row![
        container(icon(Icon::Snowflake).size(18.0).role(Role::Accent))
            .padding(5.0)
            .fill(Tone::Chrome)
            .border(Edge::Line),
        column![text("Yukimi").size(15.0).weight(700.0), text("雪見 · Software").size(11.0).role(Role::Text3),]
            .spacing(1.0),
    ]
    .spacing(10.0)
    .padding([14.0, 12.0, 12.0, 12.0]);
    let places = column(Page::ALL.iter().filter(|p| offers(app, **p)).map(|p| place(app, *p)).collect())
        .spacing(1.0)
        .padding([0.0, 6.0])
        .width(Fill);
    column![brand, divider().edge(Edge::Line), aizome_kit::caption("Places"), places, spacer(), system_card(app)]
        .width(Fill)
        .height(Fill)
        .into()
}

/// The sidebar's foot: this computer, at a glance.
fn system_card(app: &Yukimi) -> Element<Msg> {
    let model = &app.model;
    let name = if model.info.hostname.is_empty() { "This computer".to_owned() } else { model.info.hostname.clone() };
    let mut lines =
        column![row![icon(Icon::Laptop).size(14.0).role(Role::Text3), text(name).weight(650.0)].spacing(7.0)]
            .spacing(4.0);
    if !model.info.nixos_version.is_empty() {
        lines = lines.push(text(format!("NixOS {}", model.info.release())).mono().size(11.0).role(Role::Text3));
    }
    let kind = match model.setup.kind {
        yukimi_system::setup::Kind::Flake => "flake",
        yukimi_system::setup::Kind::Channels => "channels",
    };
    if !app.loading {
        lines = lines.push(text(format!("{kind} · {}", model.setup.dir.display())).mono().size(11.0).role(Role::Text3));
    }
    if app.loading || app.reading_store {
        let what = if app.loading { "Looking around…" } else { "Reading the store…" };
        lines = lines.push(text(what).size(11.0).role(Role::Text3)).push(progress_indeterminate().width(Fill));
    } else if let Some(store) = &model.store {
        let c = &store.composition;
        lines = lines.push(widgets::bar(storage::segments(c), 6.0).width(Fill));
    }
    container(lines.width(Fill)).padding(12.0).edges(Edge::Line, Sides::TOP).width(Fill).into()
}

fn tool_bar(app: &Yukimi) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> =
        vec![container(text(app.page.title()).size(14.0).weight(650.0)).padding([0.0, 6.0, 0.0, 6.0]).into()];
    if let Some(sub) = subtitle(app) {
        items.push(status_text(sub));
    }
    items.push(spacer().into());
    items.extend(page_tools(app));
    items.push(toolbar_separator());
    items.push(
        icon_button(Icon::RefreshCw)
            .toolbar()
            .on_press_maybe((!app.loading).then_some(Msg::Reload))
            .tooltip("Read everything again (❖R)")
            .into(),
    );
    aizome_kit::toolbar(items)
}

/// A line beside the page's name.
fn subtitle(app: &Yukimi) -> Option<String> {
    let model = &app.model;
    match app.page {
        Page::Overview if !model.info.hostname.is_empty() => Some(model.info.hostname.clone()),
        Page::Discover => match (&app.index, app.indexing) {
            (Some(index), _) => Some(format!("{} packages in Nixpkgs", group_digits(index.len()))),
            (None, true) => Some("getting to know Nixpkgs…".to_owned()),
            _ => None,
        },
        Page::History if !model.generations.is_empty() => Some(format!("{} generations", model.generations.len())),
        Page::Updates => app.check.as_ref().map(|c| format!("checked {}", yukimi_system::human_age(c.when, app.now()))),
        Page::Storage => model.store.as_ref().map(|s| format!("{} paths", group_digits(s.composition.paths))),
        Page::Desktops if !app.choice_changes().is_empty() => Some("not applied yet".to_owned()),
        _ => None,
    }
}

/// The page's own controls, at the toolbar's right.
fn page_tools(app: &Yukimi) -> Vec<Element<Msg>> {
    match app.page {
        Page::Discover => vec![
            search_field(&app.query)
                .placeholder("Search applications and packages")
                .on_change(Msg::Search)
                .id("search")
                .width(300.0)
                .into(),
        ],
        Page::Updates => updates::tools(app),
        Page::Desktops => desktops::tools(app),
        Page::Storage => vec![
            button("Clean Up…")
                .toolbar()
                .icon(Icon::Trash)
                .on_press_maybe(app.model.store.is_some().then_some(Msg::CleanUp))
                .into(),
        ],
        _ => Vec::new(),
    }
}

fn status_line(app: &Yukimi) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = Vec::new();
    match app.notes.back() {
        Some((_, note)) => {
            items.push(icon(Icon::Info).size(13.0).role(Role::Accent).into());
            items.push(text(note.clone()).size(12.0).role(Role::Text2).into());
        }
        None => {
            let ready = if app.loading {
                "Looking around…"
            } else if app.reading_store {
                "Reading the store…"
            } else if app.checking {
                "Asking each source for its newest version…"
            } else if app.indexing && app.page == Page::Discover {
                "Making the list of Nixpkgs's packages, at low priority…"
            } else {
                "Ready"
            };
            items.push(text(ready).size(12.0).role(Role::Text3).into());
        }
    }
    items.push(spacer().into());
    let model = &app.model;
    let mut facts = Vec::new();
    if let Some(g) = model.current_generation() {
        facts.push(format!("generation {}", g.number));
    }
    if !model.info.kernel.is_empty() {
        facts.push(format!("Linux {}", model.info.kernel));
    }
    if let Some(store) = &model.store {
        facts.push(format!("{} store paths", group_digits(store.composition.paths)));
    }
    if model.paths.elsewhere {
        facts.push(format!("looking at {}", model.paths.etc.display()));
    }
    items.push(status_text(facts.join(" · ")));
    status_bar(items)
}

// Pieces the pages share.

/// A caption over a group: mono capitals, spaced, as the Deskbar's SHELF.
pub fn caption<Msg>(s: &str) -> Text<Msg> {
    text(s.to_owned()).mono().size(10.5).caps().letter_spacing(1.2).role(Role::Text3)
}

/// A page's heading: what it shows, and a line of explanation.
pub fn heading<Msg: 'static>(title: &str, explanation: &str) -> Element<Msg> {
    let mut c = column![text(title.to_owned()).size(17.0).weight(650.0)].spacing(4.0).width(Fill);
    if !explanation.is_empty() {
        c = c.push(text(explanation.to_owned()).wrap().role(Role::Text2).line_height(1.35));
    }
    c.into()
}

/// A panel on the page: chrome, edged, with the top highlight.
pub fn panel<Msg>(content: impl Into<Element<Msg>>) -> Container<Msg> {
    container(content).fill(Tone::Chrome).border(Edge::Line).bevel().padding(14.0)
}

/// A group of rows under a caption, with an explanation and something at
/// the caption's right (a button, a field).
pub fn group<Msg: 'static>(
    title: &str,
    description: Option<String>,
    trailing: Option<Element<Msg>>,
    rows: Vec<Element<Msg>>,
) -> Element<Msg> {
    let mut head = row![caption(title)].spacing(8.0).width(Fill);
    head = head.push(spacer());
    if let Some(t) = trailing {
        head = head.push(t);
    }
    let mut c = column![head].spacing(6.0).width(Fill);
    if let Some(d) = description {
        c = c.push(text(d).wrap().size(12.0).role(Role::Text3));
    }
    c.push(list(rows)).into()
}

/// Rows in a well, a rule between each.
pub fn list<Msg: 'static>(rows: Vec<Element<Msg>>) -> Element<Msg> {
    let mut items = Vec::with_capacity(rows.len() * 2);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            items.push(divider().edge(Edge::Line).into());
        }
        items.push(r);
    }
    container(column(items).width(Fill)).fill(Tone::Chrome).border(Edge::Line).width(Fill).into()
}

/// A row with something at its left (a monogram, an icon), a title and a
/// line under it, and things at its right.
pub fn item_row<Msg: 'static>(
    leading: Option<Element<Msg>>,
    title: Element<Msg>,
    subtitle: Option<Element<Msg>>,
    trailing: Vec<Element<Msg>>,
) -> Element<Msg> {
    let mut words = column![title].spacing(2.0).width(Fill);
    if let Some(s) = subtitle {
        words = words.push(s);
    }
    let mut main = row![].spacing(12.0).width(Fill);
    if let Some(l) = leading {
        main = main.push(l);
    }
    main = main.push(words);
    // What's at the right goes under the rest when the window is narrow.
    container(widgets::adapt(main.into(), trailing, 8.0, 200.0))
        .padding([8.0, 12.0])
        .min_height(44.0)
        .width(Fill)
        .into()
}

/// A row saying something is on its way.
pub fn pending<Msg: 'static>(what: &str) -> Element<Msg> {
    row![progress_indeterminate().width(48.0), text(what.to_owned()).role(Role::Text3)]
        .spacing(12.0)
        .padding([12.0, 12.0])
        .width(Fill)
        .into()
}

/// A row with nothing in it but a quiet line.
pub fn empty<Msg: 'static>(what: &str) -> Element<Msg> {
    row![text(what.to_owned()).role(Role::Text3).wrap()].padding([12.0, 12.0]).width(Fill).into()
}

/// A note across the page: an edge in the role's colour and the text.
pub fn banner<Msg: 'static>(icon_name: Icon, role: Role, words: String) -> Element<Msg> {
    widgets::noted(
        role,
        row![icon(icon_name).size(16.0).role(role), text(words).wrap().role(Role::Text).width(Fill)]
            .spacing(10.0)
            .width(Fill),
    )
}

/// A plain button that goes somewhere: "See what's installed →".
pub fn link<Msg: Clone + Send + 'static>(label: &str, msg: Msg) -> Element<Msg> {
    button(format!("{label} →")).plain().compact().on_press(msg).into()
}

/// Mono text for numbers, versions and sizes.
pub fn mono<Msg>(s: impl Into<String>) -> Text<Msg> {
    text(s.into()).mono().tabular().size(12.0).role(Role::Text2)
}

/// A size in two or three figures: `61 GB`, `312 MB`.
pub fn short_size(bytes: u64) -> String {
    yukimi_system::human_size(bytes)
}

/// `1234567` as `1,234,567`.
pub fn group_digits(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A chip for whether a role's colour means good, bad or news.
pub fn tag<Msg: 'static>(words: &str, role: Role) -> Element<Msg> {
    chip(words, role)
}

/// A dye's swatch, for legends.
pub fn swatch<Msg: 'static>(dye: Dye) -> Element<Msg> {
    widgets::bar(vec![(1.0, dye)], 12.0).width(12.0)
}

/// Builders passed through a function: `x.pipe(f)` is `f(x)`.
pub trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    #[test]
    fn digits_are_grouped() {
        assert_eq!(super::group_digits(123), "123");
        assert_eq!(super::group_digits(1234), "1,234");
        assert_eq!(super::group_digits(1234567), "1,234,567");
    }
}
