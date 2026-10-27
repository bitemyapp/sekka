//! The public part of a person's display settings, shared with the login
//! screen through AccountsService. Never publish the rest of settings.toml.

use crate::OutputConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DisplayScales {
    pub outputs: Vec<OutputConfig>,
}

impl DisplayScales {
    /// Keep matching rules in their original order, including entries without
    /// a scale: they can shadow a later rule, just as in the session.
    pub fn from_outputs(outputs: &[OutputConfig]) -> Self {
        Self {
            outputs: outputs
                .iter()
                .map(|o| OutputConfig {
                    name: o.name.clone(),
                    make: o.make.clone(),
                    model: o.model.clone(),
                    serial: o.serial.clone(),
                    // Settings offers 1–4×. Ignore corrupt values at login.
                    scale: o.scale.filter(|s| s.is_finite() && (1.0..=4.0).contains(s)),
                    ..Default::default()
                })
                .collect(),
        }
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self)
    }

    /// Treat the public account property as untrusted input. Only display
    /// identities and scales can enter the greeter's compositor settings.
    pub fn from_toml(text: &str) -> Option<Self> {
        if text.len() > 64 * 1024 {
            return None;
        }
        let parsed: Self = toml::from_str(text).ok()?;
        Some(Self::from_outputs(&parsed.outputs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    #[test]
    fn shares_only_scales_and_preserves_display_matching() {
        let settings = Settings::from_toml(
            r#"
[keys]
terminal = ["private-command"]
[[outputs]]
make = "AUS"
model = "PG27UCDM"
serial = "123"
scale = 2.0
enabled = false
mode = "3840x2160@240"
position = [200, 100]
mirror = "DP-2"
[[outputs]]
name = "HDMI-A-1"
scale = 1.5
"#,
        )
        .unwrap();
        let public = DisplayScales::from_outputs(&settings.outputs);
        let text = public.to_toml().unwrap();
        assert!(!text.contains("private-command"));
        assert!(!text.contains("enabled"));
        let roundtrip = DisplayScales::from_toml(&text).unwrap();
        assert_eq!(public, roundtrip);
        let login = Settings { outputs: roundtrip.outputs, ..Default::default() };
        let panel = login.output("DP-3", "AUS", "PG27UCDM", "123").unwrap();
        assert_eq!(panel.scale, Some(2.0));
        assert_eq!(panel.enabled, None);
        assert_eq!(panel.mode, None);
        assert_eq!(panel.position, None);
        assert!(panel.extra.is_empty());
        assert_eq!(login.output("HDMI-A-1", "", "", "").unwrap().scale, Some(1.5));
        assert!(login.output("DP-3", "AUS", "PG27UCDM", "other").is_none());
    }

    #[test]
    fn malformed_scales_cannot_break_login_or_enable_other_settings() {
        assert!(DisplayScales::from_toml("broken = [").is_none());
        assert!(DisplayScales::from_toml(&" ".repeat(65537)).is_none());
        for scale in ["nan", "inf", "-1.0", "0.0", "5.0"] {
            let text = format!("[[outputs]]\nname = 'DP-1'\nscale = {scale}\nenabled = false\n");
            let public = DisplayScales::from_toml(&text).unwrap();
            assert_eq!(public.outputs[0].scale, None);
            assert_eq!(public.outputs[0].enabled, None);
        }
    }

    #[test]
    fn an_unscaled_rule_still_shadows_later_rules() {
        let public =
            DisplayScales::from_toml("[[outputs]]\nname = 'DP-1'\n[[outputs]]\nname = 'DP-1'\nscale = 2.0\n").unwrap();
        let login = Settings { outputs: public.outputs, ..Default::default() };
        assert_eq!(login.output("DP-1", "", "", "").unwrap().scale, None);
    }
}
