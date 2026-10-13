//! The list of nightly builds as the download page shows it, from
//! nightly/builds.json (nix/publish-nightly.nix writes it, newest first).

use serde::Deserialize;
use std::fmt::Write;

#[derive(Deserialize)]
pub struct Builds {
    pub builds: Vec<Build>,
}

/// One build; paths are relative to the nightly directory.
#[derive(Deserialize)]
pub struct Build {
    pub id: String,
    pub revision: String,
    pub uploaded: String,
    pub iso: String,
    pub sha256sums: String,
    pub manifest: String,
    pub flake_lock: String,
    pub bytes: u64,
}

/// The page's place for the list, which holds its fallback until filled.
const START: &str = "<!--builds-->";
const END: &str = "<!--/builds-->";

pub const NONE_YET: &str = "<p>No nightly has been published yet.</p>";

/// The page with `list` in place of its fallback, or None if the page has
/// no place for it.
pub fn fill(page: &str, list: &str) -> Option<String> {
    let start = page.find(START)?;
    let end = start + page[start..].find(END)? + END.len();
    Some(format!("{}{list}{}", &page[..start], &page[end..]))
}

/// The builds, newest first and marked as such. `now` is in Unix seconds.
pub fn render(builds: &[Build], base: &str, repo: &str, now: i64) -> String {
    let mut html = String::from("<ol class=\"builds\">\n");
    for (i, build) in builds.iter().enumerate() {
        let newest = i == 0;
        let uploaded = parse_utc(&build.uploaded);
        let (day, clock, age) = match uploaded {
            Some(t) => (day(t), clock(t), age(now - t)),
            None => (build.uploaded.clone(), String::new(), String::new()),
        };
        let iso = build.iso.rsplit('/').next().unwrap_or(&build.iso);
        let short = build.revision.get(..7).unwrap_or(&build.revision);
        let _ = write!(
            html,
            r#"<li class="build{newest_class}">
  <p class="build-when">{tag}<strong>{day}</strong> <span class="build-age">{age}</span></p>
  <p class="build-what">Uploaded {clock} UTC from <a href="{repo}/commit/{revision}"><code>{short}</code></a>, {size}</p>
  <p class="build-files"><a class="build-iso" href="{base}/{iso_path}">{iso}</a> <span class="build-extra"><a href="{base}/{sums}">SHA256SUMS</a> · <a href="{base}/{manifest}">manifest.json</a> · <a href="{base}/{lock}">flake.lock</a></span></p>
</li>
"#,
            newest_class = if newest { " newest" } else { "" },
            tag = if newest {
                r#"<span class="tag tag-accent">Newest</span>"#
            } else {
                ""
            },
            day = esc(&day),
            age = esc(&age),
            clock = esc(&clock),
            revision = esc(&build.revision),
            short = esc(short),
            size = size(build.bytes),
            iso_path = esc(&build.iso),
            iso = esc(iso),
            sums = esc(&build.sha256sums),
            manifest = esc(&build.manifest),
            lock = esc(&build.flake_lock),
        );
    }
    html.push_str("</ol>");
    html
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// "2026-10-13T08:42:05Z", as the publisher writes it, in Unix seconds.
fn parse_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, min, sec) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86400 + h * 3600 + min * 60 + sec)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// days_from_civil), and back again.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

fn day(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86400));
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

fn clock(t: i64) -> String {
    let s = t.rem_euclid(86400);
    format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
}

fn age(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    let ago = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match minutes {
        0 => "just now".into(),
        1..60 => ago(minutes, "minute"),
        60..1440 => ago(minutes / 60, "hour"),
        _ => ago(minutes / 1440, "day"),
    }
}

fn size(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://downloads.sekka.computer/nightly";
    const REPO: &str = "https://github.com/bitemyapp/sekka";

    fn build(id: &str, uploaded: &str, revision: &str) -> Build {
        Build {
            id: id.into(),
            revision: revision.into(),
            uploaded: uploaded.into(),
            iso: format!("builds/{id}/sekka-nightly-{id}-x86_64-linux.iso"),
            sha256sums: format!("builds/{id}/SHA256SUMS"),
            manifest: format!("builds/{id}/manifest.json"),
            flake_lock: format!("builds/{id}/flake.lock"),
            bytes: 6_123_456_789,
        }
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        let t = parse_utc("2026-10-13T08:42:05Z").unwrap();
        assert_eq!(t, 1_791_880_925); // date -u -d 2026-10-13T08:42:05Z +%s
        assert_eq!(day(t), "13 Oct 2026");
        assert_eq!(clock(t), "08:42");
        assert_eq!(day(parse_utc("2024-02-29T23:59:59Z").unwrap()), "29 Feb 2024");
        for bad in ["", "2026-10-13 08:42:05Z", "2026-13-01T00:00:00Z", "2026-10-13T08:42:05+01:00"] {
            assert_eq!(parse_utc(bad), None, "{bad}");
        }
    }

    #[test]
    fn ages() {
        assert_eq!(age(-5), "just now");
        assert_eq!(age(59), "just now");
        assert_eq!(age(60), "1 minute ago");
        assert_eq!(age(3 * 3600 + 5), "3 hours ago");
        assert_eq!(age(86400), "1 day ago");
        assert_eq!(age(9 * 86400), "9 days ago");
    }

    #[test]
    fn lists_newest_first_and_marks_it() {
        let now = parse_utc("2026-10-14T12:00:00Z").unwrap();
        let builds = [
            build("20261014-aaaaaaa", "2026-10-14T09:00:00Z", &"a".repeat(40)),
            build("20261013-bbbbbbb", "2026-10-13T09:00:00Z", &"b".repeat(40)),
        ];
        let html = render(&builds, BASE, REPO, now);
        assert_eq!(html.matches("<li class=\"build").count(), 2);
        assert_eq!(html.matches("class=\"build newest\"").count(), 1);
        assert!(html.find("20261014-aaaaaaa") < html.find("20261013-bbbbbbb"));
        assert!(html.contains(r#"Newest</span><strong>14 Oct 2026</strong> <span class="build-age">3 hours ago</span>"#));
        assert!(html.contains("1 day ago"));
        assert!(html.contains("Uploaded 09:00 UTC"));
        assert!(html.contains(&format!(
            r#"href="{BASE}/builds/20261014-aaaaaaa/sekka-nightly-20261014-aaaaaaa-x86_64-linux.iso">sekka-nightly-20261014-aaaaaaa-x86_64-linux.iso</a>"#
        )));
        assert!(html.contains(&format!(r#"<a href="{REPO}/commit/{}"><code>aaaaaaa</code></a>"#, "a".repeat(40))));
        assert!(html.contains("6.1 GB"));
    }

    #[test]
    fn escapes_what_it_shows() {
        let mut b = build("20261014-aaaaaaa", "2026-10-14T09:00:00Z", "a<b\"c'&d");
        b.iso = "builds/x/<script>.iso".into();
        let html = render(&[b], BASE, REPO, 0);
        assert!(!html.contains("<script>") && !html.contains("a<b"));
        assert!(html.contains("&lt;script&gt;.iso") && html.contains("a&lt;b&quot;c&#39;&amp;d"));
    }

    #[test]
    fn fills_only_the_place_for_builds() {
        let page = "<main>a<!--builds--><p>fallback</p><!--/builds-->b</main>";
        assert_eq!(fill(page, "LIST").as_deref(), Some("<main>aLISTb</main>"));
        assert_eq!(fill("<main>no place</main>", "LIST"), None);
    }

    #[test]
    fn reads_what_the_publisher_writes() {
        let json = r#"{"builds": [{"id": "20261014-aaaaaaa", "revision": "aaaaaaa", "committed": "2026-10-14T03:00:00-05:00",
          "uploaded": "2026-10-14T09:00:00Z", "system": "x86_64-linux", "iso": "builds/20261014-aaaaaaa/x.iso",
          "sha256sums": "builds/20261014-aaaaaaa/SHA256SUMS", "manifest": "builds/20261014-aaaaaaa/manifest.json",
          "flake_lock": "builds/20261014-aaaaaaa/flake.lock", "bytes": 1, "sha256": "00"}]}"#;
        let builds: Builds = serde_json::from_str(json).unwrap();
        assert_eq!(builds.builds[0].id, "20261014-aaaaaaa");
    }
}
