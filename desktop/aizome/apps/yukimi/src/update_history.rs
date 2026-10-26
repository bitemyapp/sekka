//! Revision comparisons from the helper's saved generation configurations.
//! Branch names and a per-user record of updates are not needed.
use std::collections::BTreeMap;
use std::path::Path;

use yukimi_config::lock::{FlakeLock, Input, Locked, Movement};
use yukimi_system::generations::Generation;

#[derive(Clone, Debug, Default)]
pub struct History {
    snapshots: BTreeMap<u32, Vec<Input>>,
    active: Option<u32>,
    current: Option<u32>,
}

impl History {
    pub fn load(root: &Path, generations: &[Generation]) -> Self {
        Self {
            snapshots: generations
                .iter()
                .filter_map(|g| Some((g.number, read_inputs(&root.join(format!("generation-{}", g.number)))?)))
                .collect(),
            active: generations.iter().filter(|g| g.active).max_by_key(|g| (g.current, g.number)).map(|g| g.number),
            current: generations.iter().find(|g| g.current).map(|g| g.number),
        }
    }

    fn revision(&self, generation: u32, input: &Input) -> Option<&Locked> {
        let saved = self.snapshots.get(&generation)?.iter().find(|i| i.name == input.name)?.locked.as_ref()?;
        same_repo(saved, input.locked.as_ref()?).then_some(saved)
    }

    pub fn running(&self, input: &Input) -> Option<&Locked> {
        self.revision(self.active?, input)
    }

    /// The profile selection can differ from the running system after a boot
    /// update or rollback. Never assume the highest generation is selected.
    fn prepared(&self, input: &Input) -> Option<(u32, &Locked)> {
        let number = self.current?;
        (Some(number) != self.active).then_some(())?;
        Some((number, self.revision(number, input)?))
    }

    pub fn revisions(&self, input: &Input, movement: Option<&Movement>) -> String {
        let Some(pinned) = input.locked.as_ref() else { return String::new() };
        let running = self.running(input);
        let mut text = match running.and_then(Locked::short_rev) {
            Some(rev) => format!("Running: {rev}"),
            None => String::new(),
        };
        if running.is_none_or(|r| r.rev != pinned.rev)
            && let Some(rev) = pinned.short_rev()
        {
            if !text.is_empty() {
                text.push_str(" · ");
            }
            text.push_str(&format!("Pinned: {rev}"));
        }
        if let Some((_, prepared)) = self.prepared(input)
            && running.is_none_or(|r| r.rev != prepared.rev)
            && let Some(rev) = prepared.short_rev()
        {
            text.push_str(&format!(" · Prepared: {rev}"));
        }
        if let Some(Movement::Newer(target)) = movement
            && let Some(rev) = target.short_rev()
        {
            text.push_str(&format!(" → Target: {rev}"));
        }
        text
    }

    pub fn link(&self, input: &Input, movement: Option<&Movement>) -> Option<Link> {
        let pinned = input.locked.as_ref()?;
        let running = self.running(input);
        let baseline = running.unwrap_or(pinned);
        if let Some(Movement::Newer(target)) = movement {
            // Update checks return release metadata, without owner/repository.
            // checks::movements already scopes that answer to this input's source.
            let target = Locked { rev: target.rev.clone(), ..pinned.clone() };
            if let Some(link) = comparison(
                "Changes",
                &format!("Pending update: {} → target", if running.is_some() { "running" } else { "pinned" }),
                baseline,
                &target,
            ) {
                return Some(link);
            }
        }
        if let Some((number, prepared)) = self.prepared(input)
            && let Some(running) = running
            && let Some(mut link) = comparison(
                "Prepared changes",
                &format!("Running generation {} → selected generation {number}", self.active?),
                running,
                prepared,
            )
        {
            link.generations = Some((self.active?, number));
            return Some(link);
        }
        if let Some(running) = running
            && let Some(link) = comparison("Configured changes", "Running → pinned in flake.lock", running, pinned)
        {
            return Some(link);
        }
        if let Some(number) = self.active
            && let Some(running) = running
        {
            // Settings-only generations must not hide the last source change.
            // Skip unavailable snapshots, but stop at a source removal/replacement.
            // Name the actual saved endpoints instead of claiming adjacent builds.
            let mut after_number = number;
            for (&before_number, _) in self.snapshots.range(..number).rev() {
                let Some(before) = self.revision(before_number, input) else { break };
                if before.rev != running.rev {
                    if let Some(mut link) = comparison(
                        "Last update",
                        &format!("Saved generations {before_number} → {after_number}"),
                        before,
                        running,
                    ) {
                        link.generations = Some((before_number, after_number));
                        return Some(link);
                    }
                    break;
                }
                after_number = before_number;
            }
        }
        let (label, description) = if running.is_some() {
            ("Running commit", "Revision in the running generation")
        } else {
            ("Pinned commit", "Revision pinned in flake.lock; running revision unavailable")
        };
        Some(Link {
            label,
            tooltip: format!("{description}: {}. No saved comparison available.", baseline.rev.as_deref()?),
            url: baseline.web_url()?,
            before: None,
            after: baseline.clone(),
            generations: None,
        })
    }
}

pub(crate) fn same_repo(a: &Locked, b: &Locked) -> bool {
    a.kind == b.kind
        && a.host.as_deref().unwrap_or("github.com") == b.host.as_deref().unwrap_or("github.com")
        && a.owner == b.owner
        && a.repo == b.repo
        && a.url == b.url
}

/// A visible label and a tooltip that name both endpoints of the jump.
#[derive(Clone, Debug)]
pub struct Link {
    pub label: &'static str,
    pub tooltip: String,
    pub url: String,
    pub before: Option<Locked>,
    pub after: Locked,
    pub generations: Option<(u32, u32)>,
}

fn comparison(label: &'static str, description: &str, before: &Locked, after: &Locked) -> Option<Link> {
    same_repo(before, after).then_some(())?;
    Some(Link {
        label,
        url: before.compare_url(after.rev.as_deref()?)?,
        tooltip: format!("{description}: {} → {}", before.rev.as_deref()?, after.rev.as_deref()?),
        before: Some(before.clone()),
        after: after.clone(),
        generations: None,
    })
}

fn read_inputs(dir: &Path) -> Option<Vec<Input>> {
    FlakeLock::parse(&std::fs::read_to_string(dir.join("flake.lock")).ok()?).ok().map(|lock| lock.inputs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn input(rev: &str) -> Input {
        Input {
            name: "sekka".into(),
            locked: Some(Locked {
                kind: "github".into(),
                owner: Some("bitemyapp".into()),
                repo: Some("sekka".into()),
                rev: Some(rev.into()),
                ..Default::default()
            }),
            original: None,
            follows: None,
            follows_inputs: Vec::new(),
        }
    }

    fn history() -> History {
        History {
            snapshots: [(24, vec![input("aaa")]), (26, vec![input("bbb")]), (27, vec![input("bbb")])].into(),
            active: Some(27),
            current: Some(27),
        }
    }

    #[test]
    fn saved_locks_supply_history_without_branches_or_app_state() {
        let dir = std::env::temp_dir().join(format!("yukimi-saved-locks-{}", std::process::id()));
        let mut generations = crate::fixture::model().generations;
        for g in &mut generations {
            g.active = g.number == 27;
            g.current = g.number == 27;
        }
        for (number, inputs) in history().snapshots {
            let path = dir.join(format!("generation-{number}"));
            std::fs::create_dir_all(&path).unwrap();
            let lock = serde_json::json!({
                "root": "root", "nodes": {
                    "root": {"inputs": {"sekka": "sekka"}},
                    "sekka": {"locked": inputs[0].locked,
                              "original": {"type": "github", "ref": "deleted-branch"}}
                }
            });
            std::fs::write(path.join("flake.lock"), lock.to_string()).unwrap();
        }
        // An unreadable snapshot doesn't prevent using older saved endpoints.
        std::fs::create_dir_all(dir.join("generation-25")).unwrap();
        std::fs::write(dir.join("generation-25/flake.lock"), "invalid").unwrap();
        for movement in [None, Some(&Movement::Current)] {
            let loaded = History::load(&dir, &generations);
            let link = loaded.link(&input("bbb"), movement).unwrap();
            assert_eq!(link.label, "Last update");
            assert_eq!(link.url, "https://github.com/bitemyapp/sekka/compare/aaa..bbb");
            assert!(link.tooltip.contains("24 → 26"));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pending_uses_running_revision_even_if_the_lock_was_edited() {
        let history = history();
        let configured = input("ccc");
        let target = yukimi_system::updates::movement(
            configured.locked.as_ref().unwrap(),
            &yukimi_system::updates::Release { rev: Some("ddd".into()), ..Default::default() },
        );
        let link = history.link(&configured, Some(&target)).unwrap();
        assert!(link.url.ends_with("bbb..ddd"));
        assert!(link.tooltip.contains("running → target"));
        assert_eq!(history.revisions(&configured, Some(&target)), "Running: bbb · Pinned: ccc → Target: ddd");
        let link = history.link(&configured, None).unwrap();
        assert_eq!(link.label, "Configured changes");
        assert!(link.url.ends_with("bbb..ccc"));
        // Failed/unbuilt lock edits don't replace the generation history.
        assert!(history.link(&input("bbb"), None).unwrap().url.ends_with("aaa..bbb"));
    }

    #[test]
    fn prepared_and_rollback_use_selected_generations_not_the_highest() {
        let mut history = history();
        history.snapshots.insert(28, vec![input("ccc")]);
        history.current = Some(28);
        let link = history.link(&input("ccc"), None).unwrap();
        assert_eq!(link.label, "Prepared changes");
        assert!(link.url.ends_with("bbb..ccc"));
        assert_eq!(history.revisions(&input("ccc"), None), "Running: bbb · Pinned: ccc · Prepared: ccc");
        history.current = Some(24);
        assert!(history.link(&input("aaa"), None).unwrap().url.ends_with("bbb..aaa"));
        history.active = Some(24);
        assert_eq!(history.link(&input("aaa"), None).unwrap().label, "Running commit");
        history.active = Some(26);
        history.current = Some(26);
        assert!(history.link(&input("bbb"), None).unwrap().url.ends_with("aaa..bbb"));
    }

    #[test]
    fn missing_history_and_changed_repositories_do_not_invent_comparisons() {
        let pinned = input("bbb");
        assert_eq!(History::default().link(&pinned, None).unwrap().label, "Pinned commit");
        let target = Movement::Newer(input("ccc").locked.unwrap());
        assert!(History::default().link(&pinned, Some(&target)).unwrap().url.ends_with("bbb..ccc"));
        let mut history = history();
        history.snapshots.get_mut(&24).unwrap()[0].locked.as_mut().unwrap().repo = Some("different".into());
        assert_eq!(history.link(&pinned, None).unwrap().label, "Running commit");
        history.snapshots.insert(24, vec![]);
        assert_eq!(history.link(&pinned, None).unwrap().label, "Running commit");
        history.snapshots.remove(&27);
        assert_eq!(history.link(&pinned, None).unwrap().label, "Pinned commit");
        let mut unsupported = pinned.clone();
        unsupported.locked.as_mut().unwrap().kind = "path".into();
        assert!(history.link(&unsupported, None).is_none());
    }

    #[test]
    fn each_source_keeps_its_own_last_change() {
        let mut history = history();
        let mut other = input("old");
        other.name = "other".into();
        for (&number, inputs) in &mut history.snapshots {
            let mut saved = other.clone();
            if number == 27 {
                saved.locked.as_mut().unwrap().rev = Some("new".into());
            }
            inputs.push(saved);
        }
        other.locked.as_mut().unwrap().rev = Some("new".into());
        let first = history.link(&input("bbb"), None).unwrap();
        let second = history.link(&other, None).unwrap();
        assert!(first.url.ends_with("aaa..bbb"));
        assert!(first.tooltip.contains("24 → 26"));
        assert!(second.url.ends_with("old..new"));
        assert!(second.tooltip.contains("26 → 27"));
    }

    #[test]
    fn identical_system_builds_prefer_the_selected_active_generation() {
        let mut generations = crate::fixture::model().generations;
        for g in &mut generations {
            g.active = matches!(g.number, 26 | 28);
            g.current = g.number == 26;
        }
        let missing = Path::new("/nonexistent-yukimi-snapshots");
        assert_eq!(History::load(missing, &generations).active, Some(26));
        for g in &mut generations {
            g.current = g.number == 27;
        }
        assert_eq!(History::load(missing, &generations).active, Some(28));
    }

    #[test]
    fn completed_update_is_visible_without_an_update_check() {
        use crate::app::{Clock, Msg, Page, Runner, Source, Yukimi};
        use aizome_kit::testing::Harness;

        let mut model = crate::fixture::model();
        let after = model.inputs.iter().find(|i| i.name == "sekka").unwrap().clone();
        let mut before = after.clone();
        before.locked.as_mut().unwrap().rev = Some("previous".into());
        model.update_history =
            History { snapshots: [(27, vec![before]), (28, vec![after])].into(), active: Some(28), current: Some(28) };
        let mut app = Yukimi::with(
            Source::Sample(Arc::new(model)),
            Runner::Mock(Arc::new(|_| Vec::new())),
            Clock::Fixed(crate::fixture::NOW, crate::fixture::HOUR),
        );
        app.auto_check = false;
        let mut harness = Harness::new(app);
        let window = harness.window();
        harness.send(Msg::Show(Page::Updates));
        assert!(harness.texts(window).iter().any(|t| t == "Last update"));
        assert!(harness.texts(window).iter().any(|t| t.starts_with("Running:")));
        if let Some(dir) = std::env::var_os("YUKIMI_PICTURES") {
            std::fs::create_dir_all(&dir).unwrap();
            harness.save_png(window, std::path::PathBuf::from(dir).join("updates-completed.png"), 1.0);
        }
    }
}
