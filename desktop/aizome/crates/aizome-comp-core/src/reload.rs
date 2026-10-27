//! Reloading the compositor's logic in place, keeping every client and
//! window: the recovery overlay's "Reload Aizome" (and `aizomectl
//! recovery-action reload-compositor`).

use crate::requests::{ReloadArgs, reload_summary};
use crate::state::State;

/// Reloads the compositor's logic (the installed one, even if it's the
/// build running: a fresh start from its saved state, or back after it was
/// set aside); says what happened.
pub fn reload_compositor(state: &mut State) -> Result<String, String> {
    if state.lg.in_call() {
        // Logic code is on the stack: at the end of this iteration.
        state.lg.pending_reload = Some(ReloadArgs { logic: true, ..Default::default() });
        return Ok("Reloading Aizome".into());
    }
    let report = state.reload(&ReloadArgs { logic: true, ..Default::default() });
    let text = reload_summary(&report);
    match report.logic.action.as_str() {
        "reloaded" => Ok(text),
        _ => Err(text),
    }
}

impl State {
    /// Reloads at the end of this loop iteration, out of any logic call (a
    /// logic's own binding for `reload`, or a key): says what happened in
    /// a notification.
    pub fn reload_soon(&mut self, args: ReloadArgs) {
        self.lg.pending_reload = Some(args);
    }
}
