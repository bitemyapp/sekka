//! An operation under way, in a sheet: what Nix is doing right now, how
//! far along it is, and its log, which closes itself when everything
//! worked and stays with the error when it did not.
use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::app::{Msg, Yukimi};
use crate::job::Outcome;

/// How many of the log's last lines show: its tail, as a terminal's.
const LOG_SHOWN: usize = 14;

pub fn sheet(app: &Yukimi) -> Element<Msg> {
    let Some(job) = &app.job else {
        return container(text("Nothing is running.")).padding(20.0).into();
    };
    let running = job.running();
    let title_icon = match &job.outcome {
        None => Icon::Hourglass,
        Some(Outcome::Done(_)) => Icon::CircleCheck,
        Some(Outcome::Failed(_)) => Icon::CircleX,
    };
    let title_role = match &job.outcome {
        None => Role::Accent,
        Some(Outcome::Done(_)) => Role::Success,
        Some(Outcome::Failed(_)) => Role::Danger,
    };
    let mut c = column![
        row![icon(title_icon).size(20.0).role(title_role), text(job.operation.title()).size(17.0).weight(700.0)]
            .spacing(10.0),
        text(job.status.clone()).wrap().size(14.0).weight(500.0),
    ]
    .spacing(12.0)
    .width(Fill);
    let bar: Element<Msg> = match job.fraction {
        Some(f) => progress(f).width(Fill).into(),
        None if running => progress_indeterminate().width(Fill).into(),
        None => progress(0.0).width(Fill).into(),
    };
    c = c.push(bar);
    if !job.summary.is_empty() {
        c = c.push(text(job.summary.clone()).mono().size(12.0).role(Role::Text2));
    }
    if job.quiet(std::time::Instant::now()) {
        c = c.push(
            text(
                "Nix hasn't reported anything for a few minutes. It may be waiting on a slow download. You can wait, \
                 or stop and try again later.",
            )
            .wrap()
            .role(Role::Text3),
        );
    }
    if let Some(Outcome::Failed(why)) = &job.outcome {
        c = c.push(super::banner(Icon::CircleX, Role::Danger, why.clone()));
    }
    // The log.
    let toggle = button(if job.details { "Hide Details" } else { "Details" })
        .plain()
        .compact()
        .icon(if job.details { Icon::ChevronDown } else { Icon::ChevronRight })
        .on_press(Msg::JobDetails(!job.details));
    let copy = button("Copy Log")
        .compact()
        .plain()
        .icon(Icon::Copy)
        .on_press_maybe((!job.log.is_empty()).then_some(Msg::CopyLog));
    let mut details = column![
        row![toggle, spacer(), text(format!("{} lines", job.logged)).mono().size(11.0).role(Role::Text3), copy]
            .spacing(8.0)
            .width(Fill)
    ]
    .spacing(6.0)
    .width(Fill);
    if job.details {
        let skip = job.log.len().saturating_sub(LOG_SHOWN);
        let lines: Vec<Element<Msg>> =
            job.log.iter().skip(skip).map(|l| text(l.clone()).mono().size(11.0).role(Role::Text2).into()).collect();
        let log = if lines.is_empty() {
            column![text("Nothing logged yet.").mono().size(11.0).role(Role::Text3)]
        } else {
            column(lines)
        };
        details = details.push(
            container(log.spacing(1.0).padding(8.0).width(Fill).height(232.0))
                .clip()
                .fill(Tone::Sunk)
                .border(Edge::Line)
                .width(Fill),
        );
    }
    c = c.push(details);
    let buttons = if running {
        row![spacer(), button("Stop").destructive().on_press_maybe((!job.stop.requested()).then_some(Msg::StopJob))]
    } else if job.retry_authentication {
        row![
            spacer(),
            button("Close").on_press(Msg::CloseJob),
            button("Try Again").primary().default().on_press(Msg::RetryAuthentication)
        ]
    } else {
        row![spacer(), button("Close").primary().default().on_press(Msg::CloseJob)]
    };
    c = c.push(buttons.width(Fill));
    container(c).padding(20.0).width(600.0).into()
}
