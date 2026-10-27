//! Mutually exclusive modifier arrangements, expressed as XKB options.
//! Caps Lock, Compose and layout switching are configured separately.

use super::{options, replace_options};

pub const CHOICES: &[(&str, &str)] = &[
    ("Default", ""),
    ("Swap Alt and Super", "altwin:swap_alt_win"),
    ("Swap left Alt and Super", "altwin:swap_lalt_lwin"),
    ("Swap right Alt and Super", "altwin:swap_ralt_rwin"),
    ("Swap Control and Super", "ctrl:swap_lwin_lctl,ctrl:swap_rwin_rctl"),
    ("Swap left Control and Super", "ctrl:swap_lwin_lctl"),
    ("Swap right Control and Super", "ctrl:swap_rwin_rctl"),
    ("Swap Control and Alt", "ctrl:swap_lalt_lctl,ctrl:swap_ralt_rctl"),
    ("Swap left Control and Alt", "ctrl:swap_lalt_lctl"),
    ("Swap right Control and Alt", "ctrl:swap_ralt_rctl"),
];

fn is_modifier_option(o: &str) -> bool {
    o.starts_with("altwin:") || o.starts_with("ctrl:swap_")
}

/// Unknown or combined arrangements appear as Custom, without modifying them.
pub fn selected(s: &str) -> usize {
    let mut current: Vec<_> = options(s).into_iter().filter(|o| is_modifier_option(o)).collect();
    current.sort();
    current.dedup();
    CHOICES
        .iter()
        .position(|(_, value)| {
            let mut expected = options(value);
            expected.sort();
            current == expected
        })
        .unwrap_or(CHOICES.len())
}

pub fn set(s: &str, index: usize) -> String {
    let Some((_, value)) = CHOICES.get(index) else { return s.to_owned() };
    replace_options(s, is_modifier_option, if value.is_empty() { None } else { Some(value) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_replace_conflicts_and_preserve_other_keyboard_settings() {
        let original = "grp:alt_shift_toggle,ctrl:nocaps,compose:ralt,lv3:ralt_switch,altwin:ctrl_win";
        let swapped = set(original, 1);
        assert_eq!(swapped, "grp:alt_shift_toggle,ctrl:nocaps,compose:ralt,lv3:ralt_switch,altwin:swap_alt_win");
        assert_eq!(selected(&swapped), 1);
        assert_eq!(set(&swapped, 0), "grp:alt_shift_toggle,ctrl:nocaps,compose:ralt,lv3:ralt_switch");
        assert_eq!(set("ctrl:swapcaps,ctrl:swap_lwin_lctl", 1), "ctrl:swapcaps,altwin:swap_alt_win");
    }

    #[test]
    fn arrangements_round_trip_and_compare_options_independent_of_order() {
        for i in 0..CHOICES.len() {
            assert_eq!(selected(&set("compose:menu,caps:escape", i)), i);
        }
        assert_eq!(selected(" ctrl:swap_rwin_rctl,compose:menu,ctrl:swap_lwin_lctl "), 4);
        assert_eq!(selected("altwin:swap_alt_win,altwin:swap_alt_win"), 1);
    }

    #[test]
    fn custom_arrangements_are_visible_and_only_replaced_by_an_explicit_choice() {
        for custom in ["altwin:meta_win", "altwin:swap_alt_win,ctrl:swap_lwin_lctl"] {
            assert_eq!(selected(custom), CHOICES.len());
            assert_eq!(set(custom, CHOICES.len()), custom);
            assert_eq!(set(custom, usize::MAX), custom);
            assert_eq!(set(custom, 0), "");
        }
    }
}
