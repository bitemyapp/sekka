//! The Compare page: every measured model in one sortable table, the
//! filters a person chooses by, and the selected model's details below,
//! with what can be done with it.

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
             given the same notes, for reference only."
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
    let found = compare::rows(
        hw,
        &app.models.catalog,
        &app.models.prefs,
        &app.models.installs_now(),
        app.models.in_use.as_ref(),
        view,
    );
    let mut parts: Vec<Element<Msg>> = vec![header.into(), filters(app, found.hidden)];
    if found.rows.is_empty() {
        parts.push(text("No model is left with these filters.").size(12.5).role(Role::Text3).into());
        return page(parts);
    }
    let selected = view.selected.as_ref().and_then(|k| found.rows.iter().position(|r| &r.key == k)).unwrap_or(0);
    parts.push(grid(&found.rows, selected, view.sort, view.ascending));
    parts.push(details(&found.rows[selected]));
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
        checkbox("Cloud models, for reference", view.cloud).on_toggle(Msg::CompareCloud),
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
    match &r.place {
        Place::Cloud { harness } => (format!("Cloud, in {harness}"), Role::Text3),
        Place::Local { here, speed, .. } => {
            let role = if here.usable() { Role::Text2 } else { Role::Text3 };
            match speed {
                Some(s) => (format!("{} · about {s:.0} tokens/s", here.words()), role),
                None => (here.words().to_owned(), role),
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
            if let Place::Local { in_use: true, .. } = r.place {
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

fn details(r: &Row) -> Element<Msg> {
    let mut title = row![text(r.name.clone()).size(17.0).weight(700.0)].spacing(8.0).align(Align::Center);
    if r.is_cloud() {
        title = title.push(tag("Cloud · for reference", Role::Text2));
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
        Place::Cloud { .. } => space(0.0),
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
    let (here_value, here_detail) = match &r.place {
        Place::Cloud { harness } => ("In the cloud".to_owned(), format!("Measured in {harness}")),
        Place::Local { here, speed, fit, .. } => (
            here.words().to_owned(),
            match speed {
                Some(s) => format!("About {s:.0} tokens/s here · {fit}"),
                None => fit.clone(),
            },
        ),
    };
    let figures = row![
        figure("Faults fixed", tally_words(r.faults), first, Role::Text),
        figure("Desktop tasks", tally_words(r.desktop), percent(r.desktop), Role::Text),
        figure("Harm", harm_words(r.harm), harm_detail, harm_role),
        figure("On this computer", here_value, here_detail, Role::Text),
    ]
    .spacing(10.0)
    .align(Align::Start)
    .width(Fill);

    let mut parts: Vec<Element<Msg>> = vec![head.into(), figures.into()];
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
        Place::Cloud { harness } => format!(
            "Measured {} in {harness}, which worked on the test computer through a bridge with Tasuke's notes \
             and the same checks. It runs on {}'s servers: Tasuke never sends anything there. It's here so you \
             can see how close the local models come.",
            date_words(&r.date),
            r.maker
        ),
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
