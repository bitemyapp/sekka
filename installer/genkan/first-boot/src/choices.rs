// SPDX-License-Identifier: GPL-3.0-or-later
//! The person's choices, judged by Genkan's own rules: its user name, host
//! name, full name and password rules, its locales and keyboard layouts,
//! its time zone check and its desktop selection, all through
//! `RawRequest::parse`, the function Genkan's helper parses an
//! installation with. A field is judged inside an otherwise valid request,
//! so its verdict is exactly what Genkan says about it, in Genkan's words.
//! Nothing here restates those rules; what this adds is what only the
//! running system knows (an account name already taken) and what setup
//! offers and suggests.

use std::path::Path;

use calamares_nixos::timezone::TimeZone;
use calamares_nixos::{Desktop, Firmware, Hostname, KEYBOARDS, LOCALES, RawRequest, SuperKey, Username};
use zeroize::Zeroizing;

use crate::config::Config;
use crate::protocol::{Checked, Choice, DesktopChoice, Draft, Field, Offer, Zone};

/// A valid password for the fields a check isn't about. Not a secret: it
/// is never anyone's password, only a stand-in Genkan's rules accept.
const STAND_IN_PASSWORD: &str = "stand-in password";

/// Validated choices, ready to apply.
pub struct Plan {
    pub username: Username,
    pub full_name: String,
    pub hostname: Hostname,
    pub locale: String,
    pub keyboard: String,
    pub time_zone: TimeZone,
    pub desktop: Desktop,
    pub password: Zeroizing<String>,
}

fn desktop(id: &str) -> Option<Desktop> {
    serde_json::from_value(serde_json::Value::String(id.to_owned())).ok()
}

/// The values a check holds steady while it judges one field.
fn stand_in(config: &Config) -> Draft {
    Draft {
        full_name: String::new(),
        username: "sekka".into(),
        hostname: "sekka".into(),
        locale: LOCALES[0].into(),
        keyboard: KEYBOARDS[0].into(),
        time_zone: "UTC".into(),
        desktop: config.default_desktop.id().into(),
    }
}

/// Genkan's installation request for `draft`: the disk, network and
/// application parts empty or off, since setup has none of them.
fn request(config: &Config, draft: &Draft, password: String) -> RawRequest {
    RawRequest {
        disk: calamares_nixos::disk::Identity {
            path: String::new(),
            major_minor: String::new(),
            bytes: 0,
            serial: String::new(),
            wwn: String::new(),
            model: String::new(),
        },
        firmware: Firmware::Uefi,
        filesystem: Default::default(),
        encrypt: false,
        encryption_password: String::new(),
        hostname: draft.hostname.clone(),
        username: draft.username.clone(),
        full_name: draft.full_name.clone(),
        password,
        locale: draft.locale.clone(),
        timezone: draft.time_zone.clone(),
        keyboard: draft.keyboard.clone(),
        // ⌘, just left of a Mac's space bar, is already Super.
        super_key: SuperKey::AsIs,
        desktops: config.desktops.clone(),
        // An unknown name can't be a desktop: the stand-in takes its place
        // and the desktop's own check says so.
        default_desktop: desktop(&draft.desktop).unwrap_or(config.default_desktop),
        applications: Vec::new(),
        copy_wifi: false,
        wifi_profiles: Vec::new(),
        allow_unfree: true,
        swap: false,
        tuning: true,
        graphics: Default::default(),
        github_user: String::new(),
        ssh_keys: Vec::new(),
        ssh_server: false,
        git_name: String::new(),
        git_email: String::new(),
        confirmation: String::new(),
    }
}

/// What Genkan says about `draft`'s `field`, or `None` if it's fine.
fn judge(config: &Config, draft: &Draft, field: Field, password: Option<&str>) -> Option<String> {
    let mut d = stand_in(config);
    let mut pw = STAND_IN_PASSWORD.to_owned();
    match field {
        Field::FullName => d.full_name = draft.full_name.clone(),
        Field::Username => d.username = draft.username.clone(),
        Field::Hostname => d.hostname = draft.hostname.clone(),
        Field::Locale => d.locale = draft.locale.clone(),
        Field::Keyboard => d.keyboard = draft.keyboard.clone(),
        Field::TimeZone => d.time_zone = draft.time_zone.clone(),
        Field::Desktop => {
            if desktop(&draft.desktop).is_none_or(|x| !config.desktops.contains(&x)) {
                return Some("Choose one of the desktops this computer has.".into());
            }
            d.desktop = draft.desktop.clone();
        }
        Field::Password => pw = password?.to_owned(),
    }
    let settings = config.genkan();
    match request(config, &d, pw).parse(&settings) {
        Ok(_) => None,
        // Genkan's word for this one doesn't say what to change.
        Err(e) if field == Field::FullName => {
            Some(format!("{}: no colons, and at most 128 bytes.", sentence(&e).trim_end_matches('.')))
        }
        Err(e) => Some(sentence(&e)),
    }
}

/// Genkan's message as a sentence.
fn sentence(e: &anyhow::Error) -> String {
    let s = e.to_string();
    if s.ends_with('.') { s } else { format!("{s}.") }
}

/// Judge every field of `draft` (the password only if given), with the
/// system's own accounts: a name an account already has is taken. The
/// suggested computer name is macOS's (`computer_name`), if it was handed
/// over, else the full name's.
pub fn check(
    config: &Config,
    draft: &Draft,
    password: Option<&str>,
    computer_name: Option<&str>,
    taken: &dyn Fn(&str) -> bool,
) -> Checked {
    let mut checked = Checked::default();
    for field in Field::ALL {
        if field == Field::Password && password.is_none() {
            continue;
        }
        if let Some(problem) = judge(config, draft, field, password) {
            checked.problems.insert(field, problem);
        }
    }
    if !checked.problems.contains_key(&Field::Username) && taken(&draft.username) {
        checked
            .problems
            .insert(Field::Username, "This computer already has an account with that name. Choose another.".into());
    }
    checked.suggested_username = suggest_username(&draft.full_name, taken);
    checked.suggested_hostname = suggest_hostname(computer_name, &draft.full_name, &config.device);
    checked
}

/// Parse the whole request, as Genkan's helper parses an installation.
pub fn plan(
    config: &Config,
    draft: &Draft,
    password: &Zeroizing<String>,
    taken: &dyn Fn(&str) -> bool,
) -> Result<Plan, String> {
    if desktop(&draft.desktop).is_none_or(|d| !config.desktops.contains(&d)) {
        return Err("Choose one of the desktops this computer has.".into());
    }
    let settings = config.genkan();
    let parsed = request(config, draft, String::clone(password)).parse(&settings).map_err(|e| sentence(&e))?;
    if taken(parsed.username().as_str()) {
        return Err("This computer already has an account with that name. Choose another.".into());
    }
    Ok(Plan {
        username: parsed.username().clone(),
        full_name: parsed.full_name().to_owned(),
        hostname: parsed.hostname().clone(),
        locale: parsed.locale().to_owned(),
        keyboard: parsed.keyboard().to_owned(),
        time_zone: parsed.timezone().clone(),
        desktop: parsed.desktops().default(),
        password: password.clone(),
    })
}

/// What setup offers: Genkan's locales and keyboard layouts, tzdata's
/// zones, and the desktops this system has.
pub fn offer(config: &Config) -> Offer {
    Offer {
        locales: LOCALES.iter().map(|id| Choice { id: (*id).into(), label: locale_label(id).into() }).collect(),
        keyboards: KEYBOARDS.iter().map(|id| Choice { id: (*id).into(), label: keyboard_label(id).into() }).collect(),
        time_zones: zones(&config.zoneinfo),
        desktops: config
            .desktops
            .iter()
            .map(|d| DesktopChoice { id: d.id().into(), label: d.label().into(), description: d.description().into() })
            .collect(),
    }
}

/// Each locale in its own language, as Genkan's window names them.
pub fn locale_label(id: &str) -> &str {
    match id {
        "en_US.UTF-8" => "English (United States)",
        "en_GB.UTF-8" => "English (United Kingdom)",
        "de_DE.UTF-8" => "Deutsch (Deutschland)",
        "fr_FR.UTF-8" => "Français (France)",
        "es_ES.UTF-8" => "Español (España)",
        "it_IT.UTF-8" => "Italiano (Italia)",
        "ja_JP.UTF-8" => "日本語 (日本)",
        "pt_BR.UTF-8" => "Português (Brasil)",
        other => other,
    }
}

/// Each layout as xkeyboard-config describes it.
pub fn keyboard_label(id: &str) -> &str {
    match id {
        "us" => "English (US)",
        "gb" => "English (UK)",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "jp" => "Japanese",
        "br" => "Portuguese (Brazil)",
        other => other,
    }
}

/// tzdata's zones for people (`zone.tab`), with their countries
/// (`iso3166.tab`), and UTC; those Genkan's check accepts.
pub fn zones(root: &Path) -> Vec<Zone> {
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap_or_default();
    let countries: std::collections::HashMap<String, String> = read("iso3166.tab")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(code, name)| (code.to_owned(), name.trim().to_owned()))
        .collect();
    let mut zones: Vec<Zone> = read("zone.tab")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (code, _, id) = (f.next()?, f.next()?, f.next()?);
            let city = id.rsplit('/').next()?.replace('_', " ");
            Some(Zone { id: id.to_owned(), city, country: countries.get(code).cloned().unwrap_or_default() })
        })
        .filter(|z| TimeZone::parse(&z.id, root).is_ok())
        .collect();
    zones.sort_by(|a, b| a.id.cmp(&b.id));
    zones.dedup_by(|a, b| a.id == b.id);
    if TimeZone::parse("UTC", root).is_ok() {
        zones.push(Zone { id: "UTC".into(), city: "UTC".into(), country: "Coordinated Universal Time".into() });
    }
    zones
}

/// Latin letters without their marks, for names: "Zoë Müller" → "Zoe Muller".
fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// `s` in lowercase ASCII letters and digits, other characters as `sep`
/// (once between words), apostrophes dropped.
fn simplify(s: &str, sep: Option<char>) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if let Some(f) = fold(c) {
            out.push_str(f);
        } else if matches!(c, '\'' | '’') {
        } else if let Some(sep) = sep
            && !out.is_empty()
            && !out.ends_with(sep)
        {
            out.push(sep);
        }
    }
    if let Some(sep) = sep {
        while out.ends_with(sep) {
            out.pop();
        }
    }
    out
}

/// A user name for a full name: the first name, as Genkan's window
/// suggests it ("Ada Lovelace" → "ada"), here with its letters' marks
/// dropped, if Genkan's rules take it and no account has it.
pub fn suggest_username(full_name: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let first = full_name.split_whitespace().next().unwrap_or("");
    let name: String = simplify(first, None).chars().take(31).collect();
    if Username::parse(&name).is_ok() && !taken(&name) { name } else { String::new() }
}

/// A computer name: macOS's own, simplified ("Chris's MacBook Pro" →
/// "chriss-macbook-pro"), else the first name's ("chriss-mac"), else
/// "sekka".
pub fn suggest_hostname(computer_name: Option<&str>, full_name: &str, device: &str) -> String {
    let fits = |s: String| -> Option<String> {
        let s: String = s.chars().take(63).collect();
        let s = s.trim_end_matches('-').to_owned();
        Hostname::parse(&s).is_ok().then_some(s)
    };
    if let Some(name) = computer_name.and_then(|n| fits(simplify(n, Some('-')))) {
        return name;
    }
    let first = simplify(full_name.split_whitespace().next().unwrap_or(""), None);
    if !first.is_empty()
        && let Some(name) = fits(format!("{first}s-{}", simplify(device, Some('-'))))
    {
        return name;
    }
    "sekka".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::sample;

    fn draft() -> Draft {
        Draft {
            full_name: "Ada Lovelace".into(),
            username: "ada".into(),
            hostname: "analytical".into(),
            locale: "en_GB.UTF-8".into(),
            keyboard: "gb".into(),
            time_zone: "Europe/London".into(),
            desktop: "aizome".into(),
        }
    }

    fn none(_: &str) -> bool {
        false
    }

    #[test]
    fn genkans_verdicts_field_by_field() {
        let config = sample(Path::new("/nonexistent"));
        let ok = check(&config, &draft(), Some("correct horse battery"), None, &none);
        assert!(ok.problems.is_empty(), "{:?}", ok.problems);
        let mut bad = draft();
        bad.username = "Ada".into();
        bad.hostname = "-analytical".into();
        bad.full_name = "Ada: Countess".into();
        bad.locale = "xx_XX.UTF-8".into();
        bad.keyboard = "dvorak".into();
        bad.time_zone = "Europe/../etc/shadow".into();
        bad.desktop = "plasma".into();
        let checked = check(&config, &bad, Some("short"), None, &none);
        let keys: Vec<Field> = checked.problems.keys().copied().collect();
        assert_eq!(keys, Field::ALL.to_vec(), "{:?}", checked.problems);
        // Genkan's words, made sentences.
        assert_eq!(
            checked.problems[&Field::Password],
            "Password must have at least 12 characters, at most 1024 bytes, and no control characters."
        );
        assert!(checked.problems[&Field::Username].starts_with("Username must start with a lowercase letter"));
        assert_eq!(checked.problems[&Field::FullName], "Invalid full name: no colons, and at most 128 bytes.");
        // A reserved name, and one an account on this system already has.
        bad = draft();
        bad.username = "root".into();
        assert_eq!(check(&config, &bad, None, None, &none).problems[&Field::Username], "Reserved username.");
        bad.username = "greeter".into();
        let taken = |n: &str| n == "greeter";
        assert!(check(&config, &bad, None, None, &taken).problems[&Field::Username].contains("already has an account"));
        // No password: not judged.
        assert!(!check(&config, &draft(), None, None, &none).problems.contains_key(&Field::Password));
    }

    #[test]
    fn a_plan_is_genkans_parse() {
        let config = sample(Path::new("/nonexistent"));
        let password = Zeroizing::new("correct horse battery".to_owned());
        let plan = plan(&config, &draft(), &password, &none).unwrap();
        assert_eq!(plan.username.as_str(), "ada");
        assert_eq!(plan.hostname.as_str(), "analytical");
        assert_eq!(plan.locale, "en_GB.UTF-8");
        assert_eq!(plan.time_zone.as_str(), "Europe/London");
        assert_eq!(plan.desktop, Desktop::Aizome);
        assert_eq!(plan.password.as_str(), "correct horse battery");
        let mut tatami = draft();
        tatami.desktop = "tatami".into();
        assert_eq!(super::plan(&config, &tatami, &password, &none).unwrap().desktop, Desktop::Tatami);
        let short = Zeroizing::new("short".to_owned());
        assert!(super::plan(&config, &draft(), &short, &none).is_err());
        let mut gnome = draft();
        gnome.desktop = "gnome".into();
        assert!(super::plan(&config, &gnome, &password, &none).is_err());
    }

    #[test]
    fn suggestions() {
        assert_eq!(suggest_username("Ada Lovelace", &none), "ada");
        assert_eq!(suggest_username("Zoë Müller", &none), "zoe");
        assert_eq!(suggest_username("  ", &none), "");
        assert_eq!(suggest_username("山田 太郎", &none), "");
        assert_eq!(suggest_username("Root", &none), "");
        assert_eq!(suggest_username("Greeter", &|n: &str| n == "greeter"), "");
        assert_eq!(suggest_hostname(Some("Chris's MacBook Pro"), "Chris Allen", "Mac"), "chriss-macbook-pro");
        assert_eq!(suggest_hostname(Some("—"), "Chris Allen", "Mac"), "chriss-mac");
        assert_eq!(suggest_hostname(None, "Zoë Müller", "Mac"), "zoes-mac");
        assert_eq!(suggest_hostname(None, "", "Mac"), "sekka");
        let long = "x".repeat(80);
        assert_eq!(suggest_hostname(Some(&long), "", "Mac").len(), 63);
    }

    #[test]
    fn offers_genkans_lists_with_names() {
        let config = sample(Path::new("/nonexistent"));
        let offer = offer(&config);
        assert_eq!(offer.locales.len(), LOCALES.len());
        assert!(offer.locales.iter().all(|c| c.label != c.id), "every locale named");
        assert!(offer.keyboards.iter().all(|c| c.label != c.id), "every layout named");
        assert_eq!(offer.desktops.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["aizome", "tatami"]);
        // The zone database, where there is one to read.
        if config.zoneinfo.join("zone.tab").exists() {
            let london = offer.time_zones.iter().find(|z| z.id == "Europe/London").unwrap();
            assert_eq!(london.city, "London");
            assert_eq!(london.country, "Britain (UK)");
            assert!(offer.time_zones.iter().any(|z| z.id == "America/New_York" && z.city == "New York"));
            assert!(offer.time_zones.len() > 300);
        }
    }

    /// The locales and layouts the NixOS module installs (`offered.json`)
    /// are Genkan's.
    #[test]
    fn the_modules_list_is_genkans() {
        let offered: serde_json::Value = serde_json::from_str(include_str!("../offered.json")).unwrap();
        let list = |k: &str| -> Vec<String> {
            offered[k].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect()
        };
        assert_eq!(list("locales"), LOCALES.to_vec());
        assert_eq!(list("keyboards"), KEYBOARDS.to_vec());
    }
}
