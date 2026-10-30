//! The sidebar: the journal's places (all of it, problems, your session,
//! the kernel, crashes, logins), the sources that wrote most, counted
//! with the rest of the filter, and the log files under /var/log.

use aizome_kit::prelude::*;
use aizome_kit::widgets::layout::column as column_of;

use crate::app::{Msg, Nisshi, Unlock};
use crate::filter::{Chip, Scope};

/// Sources listed before "Show All".
const FEW: usize = 12;

pub fn view(n: &Nisshi) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = vec![caption("Journal")];
    let journal = !n.in_file();
    for s in Scope::JOURNAL {
        let count = n.view.scope_count(&s).filter(|_| journal).map(crate::number);
        let mut item =
            sidebar_item(s.icon(), s.title(), journal && n.filter.scope == s).on_press(Msg::Scope(s.clone()));
        if let Some(c) = count {
            item = item.detail(c);
        }
        items.push(item.tooltip(s.about()).key(s.title()).into());
    }
    if journal && !n.view.sources.is_empty() {
        items.push(caption("Sources"));
        let shown = n.filter.source().cloned();
        let all = n.view.sources.len();
        let many = if n.all_sources { all } else { FEW.min(all) };
        for s in n.view.sources.iter().take(many) {
            let selected = shown.as_ref() == Some(&s.name);
            let glyph = if s.errors > 0 { Icon::TriangleAlert } else { Icon::AppWindow };
            let tip = if s.problems > 0 {
                format!(
                    "{}: {} ({} warnings or worse)",
                    s.name,
                    crate::count(s.count, "message", "messages"),
                    crate::number(s.problems)
                )
            } else {
                format!("{}: {}", s.name, crate::count(s.count, "message", "messages"))
            };
            let press = if selected {
                // Pressed again, it shows every source.
                let at = n.filter.chips.iter().position(|c| matches!(c, Chip::Source { not: false, .. })).unwrap_or(0);
                Msg::RemoveChip(at)
            } else {
                Msg::OnlySource(s.name.clone())
            };
            items.push(
                sidebar_item(glyph, s.name.to_string(), selected)
                    .detail(crate::number(s.count))
                    .on_press(press)
                    .tooltip(tip)
                    .key(format!("source {}", s.name))
                    .into(),
            );
        }
        if all > FEW {
            items.push(
                sidebar_item(
                    None,
                    if n.all_sources { "Fewer".to_owned() } else { format!("All {} Sources", crate::number(all)) },
                    false,
                )
                .on_press(Msg::AllSources(!n.all_sources))
                .key("all sources")
                .into(),
            );
        }
    }
    if !n.files.is_empty() {
        items.push(caption("Log Files"));
        for f in &n.files {
            let scope = Scope::File(f.path.clone());
            let locked = !f.readable && n.unlock != Unlock::Unlocked;
            // Under /var/log, its path there; elsewhere (a stand-in), its name.
            let name = f.path.strip_prefix("/var/log/").map(str::to_owned).unwrap_or_else(|| {
                std::path::Path::new(&f.path)
                    .file_name()
                    .map_or_else(|| f.path.clone(), |n| n.to_string_lossy().into_owned())
            });
            items.push(
                sidebar_item(if locked { Icon::Lock } else { Icon::FileText }, name, n.filter.scope == scope)
                    .detail(crate::bytes(f.size))
                    .on_press(Msg::OpenFile(f.path.clone()))
                    .tooltip(if locked {
                        format!("{}: only an administrator can read it", f.path)
                    } else {
                        format!("{}, changed {}", f.path, n.clock.ago(f.modified))
                    })
                    .key(format!("file {}", f.path))
                    .into(),
            );
        }
    }
    scroll(column_of(items).width(Length::Fill).padding([0.0, 0.0, 10.0, 0.0]))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
