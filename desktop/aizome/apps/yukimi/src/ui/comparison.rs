//! A direct source diff, changed input drill-down, and system package changes.
use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::app::{ComparisonSheet, Msg};
use crate::comparison::Delta;
use yukimi_system::diff::ChangeKind;

fn commit_buttons(node: &Delta) -> Vec<Element<Msg>> {
    if let Some(url) = node.url() {
        return vec![button("Open commit diff").icon(Icon::ExternalLink).on_press(Msg::OpenUrl(url)).into()];
    }
    let mut buttons = vec![];
    for (label, source) in [("Before commit", &node.before), ("After commit", &node.after)] {
        if let Some(url) = source.as_ref().and_then(|s| s.web_url()) {
            buttons.push(button(label).compact().icon(Icon::ExternalLink).on_press(Msg::OpenUrl(url)).into());
        }
    }
    buttons
}

fn sources(comparison: &ComparisonSheet) -> Element<Msg> {
    let Some(details) = &comparison.details else { return super::pending("Reading the inputs at these revisions…") };
    let Some(node) = comparison.node() else { return super::empty("This input is no longer available.") };
    let mut names = vec![details.root.name.clone()];
    let mut selected = &details.root;
    for &index in &comparison.path {
        if let Some(child) = selected.children.get(index) {
            names.push(child.name.clone());
            selected = child;
        }
    }
    let mut top = row![].spacing(8.0).width(Fill);
    if !comparison.path.is_empty() {
        top = top.push(button("Back").icon(Icon::ChevronLeft).on_press(Msg::ComparisonBack));
    }
    top = top.push(text(names.join(" / ")).size(16.0).weight(650.0).wrap().width(Fill));
    let repository = node
        .after
        .as_ref()
        .or(node.before.as_ref())
        .map(|s| match (&s.owner, &s.repo) {
            (Some(owner), Some(repo)) => format!("{owner}/{repo}"),
            _ => s.describe(),
        })
        .unwrap_or_default();
    let mut content = column![
        top,
        text(format!("{repository} · {}", node.versions())).mono().size(12.0).role(Role::Text2).wrap(),
        row(commit_buttons(node)).spacing(8.0),
        super::caption("Changed inputs"),
    ]
    .spacing(10.0)
    .width(Fill);
    if let Some(error) = &details.source_error {
        content = content.push(text(format!("Input details unavailable: {error}")).wrap().role(Role::Text3));
    } else if node.children.is_empty() {
        content = content.push(
            text("No changed inputs in these lock files. Open the commit diff to inspect this source's changes.")
                .wrap()
                .role(Role::Text3),
        );
    } else {
        let rows = node
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let mut actions = commit_buttons(child);
                actions.push(
                    button("Inspect")
                        .compact()
                        .icon(Icon::ChevronRight)
                        .on_press(Msg::ComparisonInto(index))
                        .tooltip(format!("Inspect {} and its changed inputs", child.name))
                        .into(),
                );
                let nested = match child.children.len() {
                    0 => String::new(),
                    1 => " · 1 changed input".into(),
                    n => format!(" · {n} changed inputs"),
                };
                super::item_row(
                    None,
                    text(child.name.clone()).weight(600.0).into(),
                    Some(
                        text(format!("{} · {}{nested}", child.status(), child.versions()))
                            .mono()
                            .size(11.5)
                            .role(Role::Text3)
                            .wrap()
                            .into(),
                    ),
                    actions,
                )
            })
            .collect();
        content = content.push(super::list(rows));
    }
    if !details.source_note.is_empty() {
        content = content.push(text(details.source_note.clone()).size(11.5).wrap().role(Role::Text3));
    }
    content.into()
}

fn packages(comparison: &ComparisonSheet) -> Element<Msg> {
    let Some(details) = &comparison.details else { return super::pending("Reading package changes…") };
    let mut content = column![
        text("System package changes").size(16.0).weight(650.0),
        text("Changes across the entire generation range, including dependencies. Rebuilt means the output changed with the same version; upstream commit IDs are not recorded for every package.")
            .wrap().size(12.0).role(Role::Text2),
    ].spacing(10.0).width(Fill);
    let changes = match &details.packages {
        Ok(changes) => changes,
        Err(reason) => return content.push(text(reason.clone()).wrap().role(Role::Text3)).into(),
    };
    content = content.push(
        text_field(&comparison.query)
            .placeholder("Filter packages")
            .on_change(Msg::ComparisonQuery)
            .id("comparison-search")
            .width(Fill),
    );
    let query = comparison.query.to_lowercase();
    let filtered: Vec<_> = changes.iter().filter(|c| c.name.to_lowercase().contains(&query)).collect();
    content = content.push(
        text(format!(
            "{} package changes{}",
            filtered.len(),
            if filtered.len() > 200 { "; showing the first 200 — filter to narrow the list" } else { "" }
        ))
        .size(12.0)
        .role(Role::Text3),
    );
    let rows = filtered
        .into_iter()
        .take(200)
        .map(|change| {
            let status = match change.kind {
                ChangeKind::Added => "Added",
                ChangeKind::Removed => "Removed",
                ChangeKind::Upgraded => "Upgraded",
                ChangeKind::Downgraded => "Downgraded",
                ChangeKind::Changed if change.before == change.after => "Rebuilt",
                ChangeKind::Changed => "Changed",
            };
            let version = |v: &[String]| if v.is_empty() { "absent".into() } else { v.join(", ") };
            super::item_row(
                None,
                text(change.name.clone()).weight(600.0).into(),
                Some(
                    text(format!("{} → {}", version(&change.before), version(&change.after)))
                        .mono()
                        .size(11.5)
                        .wrap()
                        .role(Role::Text3)
                        .into(),
                ),
                vec![super::tag(status, Role::Text2)],
            )
        })
        .collect();
    content.push(super::list(rows)).into()
}

pub fn sheet(comparison: &ComparisonSheet) -> Element<Msg> {
    let mut source_tab = button("Source changes").on_press(Msg::ComparisonPackages(false));
    let mut package_tab = button("Package changes").on_press(Msg::ComparisonPackages(true));
    if comparison.packages {
        package_tab = package_tab.primary();
    } else {
        source_tab = source_tab.primary();
    }
    let mut content = column![
        text(format!("Changes in {}", comparison.input)).size(18.0).weight(700.0),
        text(comparison.link.tooltip.clone()).size(12.0).role(Role::Text2).wrap(),
        row![source_tab, package_tab].spacing(8.0),
    ]
    .spacing(12.0)
    .width(Fill);
    // The direct link is usable immediately, even if remote input reads fail.
    if comparison.details.is_none() {
        content = content.push(
            button("Open commit diff").icon(Icon::ExternalLink).on_press(Msg::OpenUrl(comparison.link.url.clone())),
        );
    }
    content = content.push(
        scroll(if comparison.packages { packages(comparison) } else { sources(comparison) })
            .key((comparison.packages, comparison.path.clone()))
            .width(Fill)
            .height(Fill),
    );
    content =
        content.push(row![spacer(), button("Done").cancel().on_press(Msg::CloseSheet(comparison.sheet))].width(Fill));
    container(content).padding(20.0).width(700.0).height(520.0).into()
}
