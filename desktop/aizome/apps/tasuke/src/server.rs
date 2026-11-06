//! The model server as the window says it: the chip's state (ready,
//! asleep, loading, stopped) and the Models page's line, from a look at
//! the router's `/health` and `/models` (and, when it doesn't answer, at
//! its systemd unit). The look itself runs on the kit's pool every few
//! seconds while the window shows ([`crate::app`]); this is what's made of
//! it, apart from any thread.

use tasuke_models::{Health, ModelId, ModelStatus, ServedModel, UnitState};

use crate::app::ModelState;

/// One look at the server.
#[derive(Clone, Debug, PartialEq)]
pub struct Polled {
    pub health: Health,
    /// The router's models and their states (when it answers).
    pub models: Vec<ServedModel>,
    /// The unit, asked only when the server doesn't answer; `None` without
    /// a systemd user manager.
    pub unit: Option<UnitState>,
}

/// Look at the server: a few HTTP requests, so not on the UI thread.
pub fn poll(server: &tasuke_models::Server) -> Polled {
    let client = server.client();
    let health = client.health();
    let models = if health == Health::Ok { client.models().unwrap_or_default() } else { Vec::new() };
    let unit = if health == Health::Ok { None } else { server.unit_state() };
    Polled { health, models, unit }
}

fn served<'p>(p: &'p Polled, id: &ModelId) -> Option<&'p ServedModel> {
    p.models.iter().find(|m| m.id == id.0)
}

fn starting(p: &Polled) -> bool {
    p.health == Health::Loading || p.unit.as_ref().is_some_and(|u| u.active == "activating" || u.active == "reloading")
}

/// The chip's state from a look at the server. `busy`: a turn is under
/// way; `was`: the state before, kept while a turn wakes the model (the
/// router can still say "unloaded" just after the request that loads it).
pub fn chip_state(p: &Polled, in_use: Option<&ModelId>, installed: bool, busy: bool, was: ModelState) -> ModelState {
    let Some(id) = in_use.filter(|_| installed) else { return ModelState::Missing };
    let waking = busy && was == ModelState::Loading;
    match &p.health {
        Health::Loading => ModelState::Loading,
        Health::Down { .. } if starting(p) || waking => ModelState::Loading,
        Health::Down { .. } => ModelState::Off,
        Health::Ok => match served(p, id) {
            Some(m) if m.failed => ModelState::Failed,
            Some(ServedModel { status: ModelStatus::Loaded, .. }) => ModelState::Ready,
            Some(ServedModel { status: ModelStatus::Loading, .. }) => ModelState::Loading,
            _ if waking => ModelState::Loading,
            _ => ModelState::Asleep,
        },
    }
}

/// The Models page's line about the server.
pub fn line(p: &Polled, in_use: Option<(&ModelId, &str)>, idle_minutes: u32) -> String {
    let unloads = match idle_minutes {
        0 => "it stays loaded".to_owned(),
        1 => "it unloads after a minute unused".to_owned(),
        n => format!("it unloads after {n} minutes unused"),
    };
    match &p.health {
        Health::Ok => {
            let loaded: Vec<&ServedModel> = p.models.iter().filter(|m| m.status == ModelStatus::Loaded).collect();
            match (loaded.first(), in_use) {
                (Some(m), Some((id, name))) if m.id == id.0 => format!("Running · {name} is loaded; {unloads}"),
                (Some(m), _) => format!("Running · {} is loaded; {unloads}", m.id),
                (None, _) => "Running · no model is loaded; one loads when you send something".to_owned(),
            }
        }
        _ if starting(p) => "Starting…".to_owned(),
        Health::Loading => "Starting…".to_owned(),
        Health::Down { .. } if p.unit.as_ref().is_some_and(|u| !u.installed()) => {
            "Not set up: the tasuke-llm service isn't installed on this computer".to_owned()
        }
        Health::Down { .. } => "Stopped · it starts when you send something".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(status: ModelStatus) -> Polled {
        Polled {
            health: Health::Ok,
            models: vec![ServedModel { id: "gemma-4-31b".into(), status, failed: false }],
            unit: None,
        }
    }

    fn down(active: &str, load: &str) -> Polled {
        Polled {
            health: Health::Down { reason: "connection refused".into() },
            models: Vec::new(),
            unit: Some(UnitState { load: load.into(), active: active.into(), sub: String::new() }),
        }
    }

    const ID: &str = "gemma-4-31b";

    fn state(p: &Polled, busy: bool, was: ModelState) -> ModelState {
        chip_state(p, Some(&ModelId::new(ID)), true, busy, was)
    }

    #[test]
    fn the_chip_says_what_the_router_says() {
        assert_eq!(state(&up(ModelStatus::Loaded), false, ModelState::Asleep), ModelState::Ready);
        assert_eq!(state(&up(ModelStatus::Sleeping), false, ModelState::Ready), ModelState::Asleep);
        assert_eq!(state(&up(ModelStatus::Unloaded), false, ModelState::Ready), ModelState::Asleep);
        assert_eq!(state(&up(ModelStatus::Loading), false, ModelState::Asleep), ModelState::Loading);
        let mut failed = up(ModelStatus::Unloaded);
        failed.models[0].failed = true;
        assert_eq!(state(&failed, false, ModelState::Asleep), ModelState::Failed);
        assert_eq!(state(&down("inactive", "loaded"), false, ModelState::Ready), ModelState::Off);
        assert_eq!(state(&down("activating", "loaded"), false, ModelState::Off), ModelState::Loading);
        assert_eq!(chip_state(&up(ModelStatus::Loaded), None, true, false, ModelState::Ready), ModelState::Missing);
        assert_eq!(
            chip_state(&up(ModelStatus::Loaded), Some(&ModelId::new(ID)), false, false, ModelState::Ready),
            ModelState::Missing
        );
    }

    #[test]
    fn a_turn_waking_the_model_isnt_undone_by_a_look_too_early() {
        // Sent: the chip says loading; the router hasn't started yet.
        assert_eq!(state(&up(ModelStatus::Unloaded), true, ModelState::Loading), ModelState::Loading);
        // The turn is starting the server itself.
        assert_eq!(state(&down("inactive", "loaded"), true, ModelState::Loading), ModelState::Loading);
        // Not waking it: the look is believed.
        assert_eq!(state(&up(ModelStatus::Unloaded), true, ModelState::Ready), ModelState::Asleep);
    }

    #[test]
    fn the_server_line() {
        let id = ModelId::new(ID);
        let named = Some((&id, "Gemma 4 31B"));
        assert_eq!(
            line(&up(ModelStatus::Loaded), named, 10),
            "Running · Gemma 4 31B is loaded; it unloads after 10 minutes unused"
        );
        assert_eq!(line(&up(ModelStatus::Loaded), named, 0), "Running · Gemma 4 31B is loaded; it stays loaded");
        assert_eq!(
            line(&up(ModelStatus::Sleeping), named, 10),
            "Running · no model is loaded; one loads when you send something"
        );
        assert_eq!(line(&down("inactive", "loaded"), named, 10), "Stopped · it starts when you send something");
        assert_eq!(line(&down("activating", "loaded"), named, 10), "Starting…");
        assert!(line(&down("inactive", "not-found"), named, 10).starts_with("Not set up"));
    }
}
