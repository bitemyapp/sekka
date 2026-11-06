//! The Compare page: every measured model in one sortable table, the
//! filters a person chooses by, and the selected model's details below,
//! with what can be done with it: download or use it, connect its account,
//! time it here.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;
use aizome_kit::widgets::table::{Column as TableColumn, table};
use aizome_kit::{column, row};

use super::models::{install_actions, tag};
use crate::app::{Msg, Tasuke};
use crate::compare::{self, Column, Place, Row, harm_words, tally_words};
use crate::models::size;

const WIDE: f64 = 980.0;
const ROW: f64 = 34.0;
const HEADER: f64 = 30.0;

pub fn view(app: &Tasuke) -> Element<Msg> {
    let header = column![
        text("Compare Models").size(22.0).weight(700.0),
        text(
            "Every model Sekka measured, on the same work: 16 faults to find and fix and 9 desktop tasks, each \
             tried twice on a test computer. A fault counts as fixed only when the problem was gone and nothing \
             else was harmed. Local models were measured inside Tasuke; cloud models in their makers' own agents, \
             given the same notes. A cloud model can be used through your own account."
        )
        .size(13.0)
        .role(Role::Text2)
        .wrap()
        .line_height(1.4),
    ]
    .spacing(4.0);
    let Some(hw) = &app.models.hardware else {
        return page(vec![
            header.into(),
            column![text("Looking at this computer…").role(Role::Text2), progress_indeterminate().width(240.0)]
                .spacing(8.0)
                .into(),
        ]);
    };
    let view = &app.models.compare;
    let found = compare::rows(hw, &app.models.catalog, &app.models.prefs, &app.models.mine(), view);
    let mut parts: Vec<Element<Msg>> = vec![header.into(), filters(app, found.hidden)];
    if found.rows.is_empty() {
        parts.push(text("No model is left with these filters.").size(12.5).role(Role::Text3).into());
        return page(parts);
    }
    let selected = view.selected.as_ref().and_then(|k| found.rows.iter().position(|r| &r.key == k)).unwrap_or(0);
    parts.push(grid(&found.rows, selected, view.sort, view.ascending));
    parts.push(details(app, &found.rows[selected]));
    page(parts)
}

fn page(parts: Vec<Element<Msg>>) -> Element<Msg> {
    scroll(
        container(column_of(parts).spacing(18.0).max_width(WIDE))
            .align_x(Align::Center)
            .padding([24.0, 32.0, 32.0, 32.0])
            .width(Fill),
    )
    .key("compare")
    .into()
}

fn filters(app: &Tasuke, hidden: usize) -> Element<Msg> {
    let view = &app.models.compare;
    let mut line = row![
        checkbox("Only models this computer runs well", view.fits_only).on_toggle(Msg::CompareFitsOnly),
        checkbox("Cloud models", view.cloud).on_toggle(Msg::CompareCloud),
    ]
    .spacing(22.0)
    .align(Align::Center);
    if hidden > 0 {
        let words = match hidden {
            1 => "1 hidden by where it was made (see Models)".to_owned(),
            n => format!("{n} hidden by where they were made (see Models)"),
        };
        line = line.push(text(words).size(12.0).role(Role::Text3));
    }
    line.width(Fill).into()
}

/// "28 of 32" with a bar of its share, for a score cell.
fn score(t: Option<tasuke_models::Tally>) -> Element<Msg> {
    match t {
        Some(t) => row![progress(t.share()).width(54.0), text(tally_words(Some(t))).size(12.5)]
            .spacing(8.0)
            .align(Align::Center)
            .into(),
        None => text("–").size(12.5).role(Role::Text3).into(),
    }
}

fn here_words(r: &Row) -> (String, Role) {
    let timed = r.timed.as_ref().map(|m| format!("{:.0} tokens/s timed", m.decode));
    match &r.place {
        Place::Cloud { model, connected, .. } => {
            let role = if *connected { Role::Text2 } else { Role::Text3 };
            match timed {
                Some(t) => (format!("Cloud · {t}"), role),
                None => (format!("In {}'s cloud", model.provider.company()), role),
            }
        }
        Place::Local { here, speed, .. } => {
            let role = if here.usable() { Role::Text2 } else { Role::Text3 };
            match (timed, speed) {
                (Some(t), _) => (format!("{} · {t}", here.words()), role),
                (None, Some(s)) => (format!("{} · about {s:.0} tokens/s", here.words()), role),
                (None, None) => (here.words().to_owned(), role),
            }
        }
    }
}

fn cell(r: &Row, c: Column) -> Element<Msg> {
    match c {
        Column::Model => {
            let mut line = row![text(r.name.clone()).size(12.5).weight(600.0)].spacing(6.0).align(Align::Center);
            if r.is_cloud() {
                line = line.push(icon(Icon::Cloud).size(12.0).role(Role::Text3));
            }
            if let Place::Local { in_use: true, .. } | Place::Cloud { in_use: true, .. } = r.place {
                line = line.push(tag("In use", Role::Success));
            }
            line = line.push(text(r.origin.clone()).size(11.0).role(Role::Text3));
            line.into()
        }
        Column::Faults => score(r.faults),
        Column::Desktop => score(r.desktop),
        Column::Harm => {
            let role = match r.harm {
                Some(n) if n > 0 => Role::Warning,
                Some(_) => Role::Text2,
                None => Role::Text3,
            };
            text(harm_words(r.harm)).size(12.5).role(role).into()
        }
        Column::Here => {
            let (words, role) = here_words(r);
            text(words).size(12.0).role(role).into()
        }
        Column::Size => match &r.place {
            Place::Local { download, .. } => text(size(*download)).size(12.5).role(Role::Text2).into(),
            Place::Cloud { .. } => text("–").size(12.5).role(Role::Text3).into(),
        },
    }
}

fn grid(rows: &[Row], selected: usize, sort: Column, ascending: bool) -> Element<Msg> {
    let columns = vec![
        TableColumn::new(Column::Model.title()).width(250.0).min_width(180.0).sortable(),
        TableColumn::new(Column::Faults.title()).width(132.0).sortable(),
        TableColumn::new(Column::Desktop.title()).width(126.0).sortable(),
        TableColumn::new(Column::Harm.title()).width(58.0).sortable(),
        TableColumn::new(Column::Here.title()).width(204.0).sortable(),
        TableColumn::new(Column::Size.title()).width(80.0).align_end().sortable(),
    ];
    let shown: Vec<Row> = rows.to_vec();
    let keys: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
    let height = HEADER + ROW * rows.len() as f64 + 2.0;
    container(
        table(columns, shown.len(), move |r, c| cell(&shown[r], Column::ALL[c]))
            .row_height(ROW)
            .selected([selected])
            .on_select(move |picked| Msg::CompareSelect(picked.first().map(|&i| keys[i].clone())))
            .sorted(sort.index(), ascending)
            .on_sort(Msg::CompareSort)
            .height(height)
            .width(Fill),
    )
    .border(Edge::Line)
    .width(Fill)
    .into()
}

/// A labelled figure: "Faults fixed", "28 of 32", "88% · 28 on the first turn".
fn figure(label: &str, value: String, detail: String, role: Role) -> Element<Msg> {
    container(
        column![
            text(label.to_owned()).size(11.0).role(Role::Text3),
            text(value).size(17.0).weight(700.0).role(role),
            text(detail).size(11.5).role(Role::Text2).wrap().line_height(1.35),
        ]
        .spacing(3.0)
        .width(Fill),
    )
    .padding([10.0, 12.0])
    .fill(Tone::Surface)
    .border(Edge::Line)
    .width(Fill)
    .into()
}

fn percent(t: Option<tasuke_models::Tally>) -> String {
    t.map(|t| format!("{:.0}%", t.share() * 100.0)).unwrap_or_default()
}

/// "4 Nov 2026" from the catalog's "2026-11-04".
fn date_words(s: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut it = s.split('-');
    match (it.next(), it.next().and_then(|m| m.parse::<usize>().ok()), it.next().and_then(|d| d.parse::<u32>().ok())) {
        (Some(y), Some(m @ 1..=12), Some(d)) => format!("{d} {} {y}", MONTHS[m - 1]),
        _ => s.to_owned(),
    }
}

/// "5,313": a whole number in groups of three.
fn grouped(n: f64) -> String {
    let digits = format!("{:.0}", n.max(0.0));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "0.2 s", "1.2 s".
fn seconds(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// The selected model's speed here: timed, being timed, or how to time it.
fn timing(app: &Tasuke, r: &Row) -> Element<Msg> {
    let models = &app.models;
    let busy = app.generating();
    if let Some(t) = models.timing.as_ref().filter(|t| t.key == r.key) {
        return row![progress_indeterminate().width(90.0), text(t.step.clone()).size(12.0).role(Role::Text2)]
            .spacing(10.0)
            .align(Align::Center)
            .into();
    }
    let failed = models.timing_failed.as_ref().filter(|(k, _)| *k == r.key).map(|(_, e)| e.clone());
    let (glyph, role, words) = match (&r.timed, &failed, &r.place) {
        (_, Some(e), _) => (Icon::CircleAlert, Role::Warning, format!("Timing it failed: {e}")),
        (Some(m), _, _) => {
            let reading =
                m.prefill.map(|p| format!(", read the prompt at {} tokens/s", grouped(p))).unwrap_or_default();
            let on = if r.is_cloud() { format!("from here, through {}", m.on) } else { format!("on the {}", m.on) };
            (
                Icon::Gauge,
                Role::Text2,
                format!(
                    "Timed {} {on}: {:.0} tokens/s writing, first token after {}{reading}.",
                    date_words(&m.date),
                    m.decode,
                    seconds(m.first_token_ms)
                ),
            )
        }
        (None, None, Place::Local { speed, .. }) if r.can_time() => (
            Icon::Gauge,
            Role::Text3,
            match speed {
                Some(s) => {
                    format!("Not timed here yet. From its size and this computer's memory, about {s:.0} tokens/s.")
                }
                None => "Not timed here yet.".to_owned(),
            },
        ),
        (None, None, Place::Local { here, .. }) if !here.usable() => {
            (Icon::Gauge, Role::Text3, "It doesn't run well enough here to be worth timing.".to_owned())
        }
        (None, None, Place::Local { .. }) => (Icon::Gauge, Role::Text3, "Download it to time it here.".to_owned()),
        (None, None, Place::Cloud { model, connected: true, .. }) => (
            Icon::Gauge,
            Role::Text3,
            format!(
                "Not timed yet: Measure Speed asks it for a short answer through your {} account.",
                model.provider.company()
            ),
        ),
        (None, None, Place::Cloud { model, .. }) => (
            Icon::Gauge,
            Role::Text3,
            format!("Connect a {} account to use it here, or to time it.", model.provider.company()),
        ),
    };
    let mut line = row![
        super::lead(icon(glyph).size(13.0).role(role)),
        text(words).size(12.0).role(Role::Text2).wrap().line_height(1.4).width(Fill)
    ]
    .spacing(6.0)
    .align(Align::Start)
    .width(Fill);
    if r.can_time() {
        let label = if r.timed.is_some() || failed.is_some() { "Measure Again" } else { "Measure Speed" };
        line = line.push(
            button(label)
                .icon(Icon::Gauge)
                .compact()
                .enabled(!busy && models.timing.is_none())
                .on_press(Msg::Measure(r.key.clone())),
        );
    }
    line.into()
}

fn details(app: &Tasuke, r: &Row) -> Element<Msg> {
    let mut title = row![text(r.name.clone()).size(17.0).weight(700.0)].spacing(8.0).align(Align::Center);
    if r.is_cloud() {
        title = title.push(tag("Cloud", Role::Text2));
    }
    if r.vision {
        title = title.push(tag("Sees images", Role::Text2));
    }
    let mut who = format!("By {} · {} · {}", r.maker, r.lineage, r.origin);
    if let Some(l) = &r.licence {
        who.push_str(&format!(" · {l} licence"));
    }
    let action: Element<Msg> = match &r.place {
        Place::Local { id, install, in_use, download, .. } => install_actions(id, install, *in_use, *download),
        Place::Cloud { in_use: true, .. } => tag("In use", Role::Success),
        Place::Cloud { model, connected: true, .. } => {
            button("Use").primary().on_press(Msg::UseCloud(model.clone())).into()
        }
        Place::Cloud { model, .. } => button(format!("Connect {}…", model.provider.company()))
            .icon(Icon::Cloud)
            .on_press(Msg::Connect(model.provider, None))
            .into(),
    };
    let head = row![
        column![title, text(who).size(12.0).role(Role::Text3).wrap().line_height(1.4)].spacing(4.0).width(Fill),
        action,
    ]
    .spacing(12.0)
    .align(Align::Start);

    let first = match (r.first_turn, r.faults) {
        (Some(f), Some(_)) => format!("{} · {} before it asked to change anything", percent(r.faults), f.passed),
        (None, Some(_)) if r.is_cloud() => format!("{} · it needed no one's yes", percent(r.faults)),
        _ => percent(r.faults),
    };
    let harm_role = if r.harm.is_some_and(|h| h > 0) { Role::Warning } else { Role::Text };
    let harm_detail = match r.harm {
        Some(0) => "Nothing else on the computer was damaged".to_owned(),
        Some(_) => "See below for what it did".to_owned(),
        None => "Not measured".to_owned(),
    };
    let (here_label, here_value, here_detail) = match &r.place {
        Place::Cloud { model, connected, .. } => (
            "Where it runs",
            format!("{}'s cloud", model.provider.company()),
            if *connected { "Through your account".to_owned() } else { "Needs your own account".to_owned() },
        ),
        Place::Local { here, speed, fit, .. } => (
            "On this computer",
            here.words().to_owned(),
            match (&r.timed, speed) {
                (Some(m), _) => format!("{:.0} tokens/s timed here · {fit}", m.decode),
                (None, Some(s)) => format!("About {s:.0} tokens/s here · {fit}"),
                (None, None) => fit.clone(),
            },
        ),
    };
    let figures = row![
        figure("Faults fixed", tally_words(r.faults), first, Role::Text),
        figure("Desktop tasks", tally_words(r.desktop), percent(r.desktop), Role::Text),
        figure("Harm", harm_words(r.harm), harm_detail, harm_role),
        figure(here_label, here_value, here_detail, Role::Text),
    ]
    .spacing(10.0)
    .align(Align::Start)
    .width(Fill);

    let mut parts: Vec<Element<Msg>> = vec![head.into(), figures.into(), timing(app, r)];
    let notes: Vec<Element<Msg>> = r
        .incidents
        .iter()
        .map(|i| (Icon::TriangleAlert, Role::Warning, i.clone()))
        .chain(r.notes.iter().map(|n| (Icon::Dot, Role::Text3, n.clone())))
        .map(|(g, role, n)| {
            row![
                super::lead(icon(g).size(13.0).role(role)),
                text(n).size(12.0).role(Role::Text2).wrap().line_height(1.4)
            ]
            .spacing(6.0)
            .align(Align::Start)
            .into()
        })
        .collect();
    if !notes.is_empty() {
        parts.push(column_of(notes).spacing(4.0).width(Fill).into());
    }
    let how = match &r.place {
        Place::Cloud { harness, model, .. } => {
            let company = model.provider.company();
            format!(
                "Measured {} in {harness}, which worked on the test computer through a bridge with Tasuke's notes \
                 and the same checks. With your own {company} account Tasuke can use it too, billed by {company}: \
                 while it's in use, what you ask and what Tasuke sees while it works is sent to {company}.",
                date_words(&r.date),
            )
        }
        Place::Local { .. } => format!(
            "Measured {} inside Tasuke: its own prompt, skills and command checks, with a test person saying \
             yes when it asked to change something.",
            date_words(&r.date)
        ),
    };
    parts.push(text(how).size(11.5).role(Role::Text3).wrap().line_height(1.4).into());
    container(column_of(parts).spacing(12.0).width(Fill))
        .padding([14.0, 16.0])
        .fill(Tone::Chrome)
        .border(Edge::Line2)
        .width(Fill)
        .key(r.key.clone())
        .into()
}

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn numbers_group_by_thousands() {
        assert_eq!(grouped(5313.4), "5,313");
        assert_eq!(grouped(812.0), "812");
        assert_eq!(grouped(1_234_567.0), "1,234,567");
    }
}
