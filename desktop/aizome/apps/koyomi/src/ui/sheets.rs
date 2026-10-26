//! The import and settings sheets, and the menus.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::app::{ImportSheet, Koyomi, Msg, View};
use crate::edit::Scope;
use crate::settings::{Clock, FirstDay};
use crate::ui::editor::{ALL_DAY_ALERTS, TIMED_ALERTS};

pub fn import(k: &Koyomi, imp: &ImportSheet) -> Element<Msg> {
    let n = imp.parsed.events.len();
    let files = match imp.files.as_slice() {
        [one] => format!("“{}”", one.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
        many => format!("{} files", many.len()),
    };
    let mut targets: Vec<String> = k.store.calendars.iter().map(|c| c.name.clone()).collect();
    targets.push(format!("New Calendar “{}”", imp.parsed.name));
    let first = imp.parsed.events.iter().map(|e| e.start.civil().date()).min();
    let last = imp.parsed.events.iter().map(|e| e.start.civil().date()).max();
    let span = match (first, last) {
        (Some(a), Some(z)) if a == z => crate::format::long_date(a),
        (Some(a), Some(z)) => {
            format!("{} {} – {} {}", crate::format::short_date(a), a.year(), crate::format::short_date(z), z.year())
        }
        _ => String::new(),
    };
    let mut info = format!("{n} event{} from {files}", if n == 1 { "" } else { "s" });
    if imp.skipped > 0 {
        info.push_str(&format!(" ({} without a date will be skipped)", imp.skipped));
    }
    column![
        text("Import Events").semibold().size(15.0),
        text(info).size(13.0).wrap(),
        text(span).mono().size(11.0).role(Role::Text3),
        form_section(
            "",
            vec![
                form_row(
                    "Add to",
                    dropdown(targets, Some(imp.target)).on_select(Msg::ImportTarget).id("import-target")
                )
                .into()
            ]
        ),
        text("Events already in the calendar (the same UID) are updated if the file's are newer.")
            .size(11.0)
            .role(Role::Text3)
            .wrap(),
        row![
            spacer(),
            button("Cancel").cancel().on_press(Msg::ImportCancel),
            button("Import").default().on_press(Msg::ImportDone).id("import-ok")
        ]
        .spacing(8.0)
        .width(Length::Fill),
    ]
    .spacing(10.0)
    .padding(18.0)
    .width(440.0)
    .into()
}

pub fn settings(k: &Koyomi) -> Element<Msg> {
    let s = &k.settings;
    let locale_first = crate::format::weekday_name(k.locale.first_day);
    let first = match s.first_day {
        FirstDay::Locale => 0,
        FirstDay::Monday => 1,
        FirstDay::Sunday => 2,
        FirstDay::Saturday => 3,
    };
    let clock = match s.clock {
        Clock::System => 0,
        Clock::H24 => 1,
        Clock::H12 => 2,
    };
    let mut timed: Vec<String> = vec!["None".into()];
    timed.extend(TIMED_ALERTS.iter().map(|(_, l)| l.to_string()));
    let ti = s.default_alert.and_then(|m| TIMED_ALERTS.iter().position(|(x, _)| *x == m)).map(|i| i + 1).unwrap_or(0);
    let mut all: Vec<String> = vec!["None".into()];
    all.extend(ALL_DAY_ALERTS.iter().map(|(_, l)| l.to_string()));
    let ai = s
        .default_all_day_alert
        .and_then(|m| ALL_DAY_ALERTS.iter().position(|(x, _)| *x == m))
        .map(|i| i + 1)
        .unwrap_or(0);
    let scheme =
        if k.locale.min_days == 4 { "ISO 8601: week 1 has the year's first Thursday" } else { "Week 1 has 1 January" };
    column![
        text("Koyomi Settings").semibold().size(15.0),
        form_section(
            "Weeks",
            vec![
                form_row(
                    "First day of the week",
                    dropdown(
                        [
                            format!("As the locale has it ({locale_first})"),
                            "Monday".into(),
                            "Sunday".into(),
                            "Saturday".into()
                        ],
                        Some(first)
                    )
                    .on_select(|i| {
                        Msg::FirstDay(
                            [FirstDay::Locale, FirstDay::Monday, FirstDay::Sunday, FirstDay::Saturday][i.min(3)],
                        )
                    })
                )
                .description(format!("Locale: {}", crate::locale::time_locale()))
                .into(),
                form_row("Week numbers", toggle(s.week_numbers).on_toggle(Msg::WeekNumbers)).description(scheme).into(),
            ]
        ),
        form_section(
            "Times",
            vec![
                form_row(
                    "Clock",
                    dropdown(["As the menu bar's", "24-hour", "12-hour"], Some(clock))
                        .on_select(|i| Msg::ClockFormat([Clock::System, Clock::H24, Clock::H12][i.min(2)]))
                )
                .into(),
                form_row(
                    "Day views start at",
                    stepper(s.day_starts as f64).range(0.0..=20.0).step(1.0).suffix(":00").on_change(Msg::DayStarts)
                )
                .into(),
            ]
        ),
        form_section(
            "Alerts",
            vec![
                form_row("New events", dropdown(timed, Some(ti)).on_select(Msg::DefaultAlert)).into(),
                form_row("New all-day events", dropdown(all, Some(ai)).on_select(Msg::DefaultAllDayAlert)).into(),
                form_row(
                    "Snooze for",
                    stepper(s.snooze_minutes as f64).range(1.0..=120.0).step(5.0).suffix(" min").on_change(Msg::Snooze)
                )
                .description("Alerts come from the Koyomi agent (aizome-koyomi --agent), even with this window closed.")
                .into(),
            ]
        ),
        row![spacer(), button("Done").default().on_press(Msg::SettingsDone)].width(Length::Fill),
    ]
    .spacing(14.0)
    .padding(18.0)
    .width(520.0)
    .into()
}

pub fn menus(k: &Koyomi) -> MenuBar<Msg> {
    let info = aizome_kit::Program::info(k);
    let name = info.name.clone();
    let sel = k.selected.is_some();
    let app = Menu::new(name.clone())
        .item(MenuItem::role_item(format!("About {name}"), MenuRole::About))
        .separator()
        .item(MenuItem::new("Settings…", Msg::ShowSettings).shortcut(Shortcut::cmd(',')))
        .separator()
        .submenu(
            Menu::new("Services").item(MenuItem::role_item("No Services Apply", MenuRole::Services).enabled(false)),
        )
        .separator()
        .item(MenuItem::role_item(format!("Hide {name}"), MenuRole::Hide).shortcut(Shortcut::cmd('H')))
        .item(MenuItem::role_item("Hide Others", MenuRole::HideOthers).shortcut(Shortcut::cmd('H').alt()))
        .separator()
        .item(MenuItem::role_item(format!("Quit {name}"), MenuRole::Quit).shortcut(Shortcut::cmd('Q')));
    let mut export = Menu::new("Export Calendar");
    for c in &k.store.calendars {
        export = export.item(MenuItem::new(format!("{}…", c.name), Msg::Export(c.id.clone())));
    }
    let file = Menu::new("File")
        .item(MenuItem::new("New Event", Msg::NewEvent).shortcut(Shortcut::cmd('N')))
        .item(MenuItem::new("New Calendar…", Msg::NewCalendar).shortcut(Shortcut::cmd('N').shift()))
        .separator()
        .item(MenuItem::new("Import…", Msg::Import).shortcut(Shortcut::cmd('I').shift()))
        .submenu(export)
        .separator()
        .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
    let edit = Menu::new("Edit")
        .item(MenuItem::new("Undo", Msg::Undo).role(MenuRole::Undo).shortcut(Shortcut::cmd('Z')))
        .item(MenuItem::new("Redo", Msg::Redo).role(MenuRole::Redo).shortcut(Shortcut::cmd('Z').shift()))
        .separator()
        .item(MenuItem::role_item("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
        .item(MenuItem::role_item("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('C')))
        .item(MenuItem::role_item("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
        .item(MenuItem::role_item("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
        .separator()
        .item(MenuItem::new("Edit Event…", Msg::Edit).shortcut(Shortcut::cmd('I')).enabled(sel))
        .item(MenuItem::new("Duplicate Event", Msg::Duplicate).shortcut(Shortcut::cmd('D')).enabled(sel))
        .item(
            MenuItem::new("Delete Event", Msg::Delete).shortcut(Shortcut::cmd_key(ShortcutKey::Backspace)).enabled(sel),
        );
    let mut first = Menu::new("First Day of the Week");
    for (label, f) in [
        (format!("As the Locale Has It ({})", crate::format::weekday_name(k.locale.first_day)), FirstDay::Locale),
        ("Monday".to_owned(), FirstDay::Monday),
        ("Sunday".to_owned(), FirstDay::Sunday),
        ("Saturday".to_owned(), FirstDay::Saturday),
    ] {
        first = first.item(MenuItem::new(label, Msg::FirstDay(f)).checked(k.settings.first_day == f));
    }
    let view = Menu::new("View")
        .item(MenuItem::new("Day", Msg::SetView(View::Day)).shortcut(Shortcut::cmd('1')).checked(k.view == View::Day))
        .item(
            MenuItem::new("Week", Msg::SetView(View::Week)).shortcut(Shortcut::cmd('2')).checked(k.view == View::Week),
        )
        .item(
            MenuItem::new("Month", Msg::SetView(View::Month))
                .shortcut(Shortcut::cmd('3'))
                .checked(k.view == View::Month),
        )
        .item(
            MenuItem::new("Year", Msg::SetView(View::Year)).shortcut(Shortcut::cmd('4')).checked(k.view == View::Year),
        )
        .separator()
        // ← → and T work without ❖ in the calendar; ❖ works anywhere.
        .item(MenuItem::new("Previous", Msg::Prev).shortcut(Shortcut::cmd_key(ShortcutKey::Left)))
        .item(MenuItem::new("Next", Msg::Next).shortcut(Shortcut::cmd_key(ShortcutKey::Right)))
        .item(MenuItem::new("Today", Msg::Today).shortcut(Shortcut::cmd('T')))
        .separator()
        .item(
            MenuItem::new("Week Numbers", Msg::WeekNumbers(!k.settings.week_numbers)).checked(k.settings.week_numbers),
        )
        .submenu(first)
        .item(MenuItem::new("Sidebar", Msg::ToggleSidebar).shortcut(Shortcut::cmd('S').alt()).checked(k.show_sidebar));
    let _ = Scope::This;
    MenuBar::standard(&info).replace(&name, app).replace("Edit", edit).with_file_menu(file).with_menu(view)
}
