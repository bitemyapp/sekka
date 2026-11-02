//! /performance/: why Aizome is fast, and how it compares, with the
//! measurements behind it. Every number here comes from docs/performance/
//! (its README, design.md, desktops.md, terminals.md, media.md, pdf.md and
//! their data/), measured on one desktop on 2 November 2026; earlier
//! records it repeats (desktop/aizome/docs/performance.md) are named where
//! they're used. When those records change, change this with them.

use crate::chart::{Bar, Chart, label_width, other, ours};
use crate::html::{Menu, Page, esc};
use crate::pages::head;
use crate::{Site, win};
use anyhow::Result;
use std::fmt::Write;

/// The repository's docs/performance, where each section's data and the
/// way to repeat it live.
const DATA: &str = "https://github.com/bitemyapp/sekka/tree/main/docs/performance";

fn doc(file: &str, label: &str) -> String {
    format!(r#"<a href="{DATA}/{file}">{}</a>"#, esc(label))
}

fn chart(id: &str, title: &str, unit: &str, decimals: usize, bars: &[Bar], caption: &str) -> String {
    Chart { id, title, unit, decimals, bars, caption, scale: 0.0, table: true, label_w: 0.0 }.html()
}

/// A row of headline figures.
fn stats(items: &[(&str, &str)]) -> String {
    let mut s = String::from(r#"<ul class="stats">"#);
    for (value, label) in items {
        let _ = write!(
            s,
            r#"<li class="stat"><span class="stat-value">{value}</span><span class="stat-label">{label}</span></li>"#
        );
    }
    s.push_str("</ul>");
    s
}

/// The design decisions, each with the section that tests it.
const DECISIONS: &str = r##"<p class="headline">Speed is decided at the bottom, once, and kept everywhere above it.</p>
<ol class="decisions">
<li><strong>One toolkit, drawing on the processor.</strong> The shell and every app are Rust programs on one toolkit, Sukumo and Aizome's kit. Each window is drawn on the CPU by its own render thread, into shared memory the compositor shows. An app opens no GPU context and loads no GL or Vulkan driver, and the toolkit is built into the app: there is almost nothing to load. <a href="#window">The cost of a window</a>, <a href="#apps">whole apps</a>.</li>
<li><strong>Only what changed is drawn.</strong> A message updates the app's state; only the rectangles of what moved or looks different are painted again. Scrolling moves the pixels already drawn and paints only the strip that comes into view. The compositor tracks damage too: moving a window composites without drawing anything again, and an output with nothing new renders no frame. <a href="#scrolling">Scrolling</a>, <a href="#idle">the idle desktop</a>.</li>
<li><strong>Every surface says what it covers.</strong> Opaque regions let the compositor skip what is under them, and integer scales (1×, 2×, 3×) put a buffer's pixels one to one on the display's, so nothing is resampled. <a href="#compositor">The compositor's frames</a>.</li>
<li><strong>A budget of 4 ms of processor time from an event to its frame,</strong> for every part of the desktop, measured by each app's own tests and written up where it isn't met. <a href="#budget">The budget on this machine</a>.</li>
<li><strong>Nothing draws that can't be seen.</strong> A window on another Desk, or minimised, is told to draw at most once a second, and only when the screen draws anyway, so a hidden app that would draw as fast as it may draws almost nothing. <a href="#hidden">Hidden windows</a>.</li>
<li><strong>The GPU where it pays.</strong> The compositor composites with the GPU, and falls back to software by itself. Video goes from the hardware decoder to the compositor as GPU buffers, never through the player's memory or a GL context of its own; PDF pages are drawn in a worker process, one per document. <a href="#media">Media</a>, <a href="#pdf">PDF</a>.</li>
</ol>"##;

fn desktops() -> String {
    let ram = [
        ours("Aizome", 221.0, "591 MiB in use with the desktop up, 370 with none; +206 MiB in a 2 GiB VM"),
        other("GNOME", 567.0, "937 MiB in use; +560 MiB at 2 GiB"),
        other("KDE Plasma", 875.0, "1,245 MiB in use, Akonadi's database among it; +842 MiB at 2 GiB"),
    ];
    let boot = [
        ours("Aizome", 6.4, "to its own login screen, settled; the bare system reaches its console in 5.1"),
        other("GNOME", 7.6, "to GDM, settled"),
        other("KDE Plasma", 8.6, "to Plasma Login, settled"),
    ];
    let login = [
        ours("Aizome", 1.2, "Enter to the desktop settled"),
        other("GNOME", 2.0, "Enter to the overview settled"),
        other("KDE Plasma", 5.2, "Enter to the desktop settled"),
    ];
    let key = [
        ours("Aizome", 0.53, "7.0 ms at the median, 13.5 at the 99th percentile (1.02 frames)"),
        other("GNOME", 0.97, "13.0 ms, 23.6 at the 99th (1.77 frames); one press took 55 ms"),
        other("KDE Plasma", 1.01, "13.5 ms, 26.2 at the 99th (1.97 frames)"),
    ];
    format!(
        r#"<p class="headline">In a small virtual machine, Aizome boots first, logs in first, needs two fifths of GNOME's memory and a quarter of Plasma's, and puts a keystroke on screen half a frame sooner.</p>
<p>The same Sekka system three times, differing only in the desktop the installer chose, in a virtual machine with 2 processors, 4 GiB of RAM and no 3D, so all three draw on the processor, as they would in most virtual and cloud desktops; and a fourth with no desktop, for the system's own share. Three boots each, alternating, timed from QEMU's start to a settled screen; then a password login, two minutes idle, a probe window timing keystrokes to the frames shown at 75 Hz, with and without two busy threads, and each desktop's own apps started cold and warm.</p>
<div class="charts">{ram}{key}{boot}{login}</div>
<div class="table-wrap"><table class="numbers"><caption>Each desktop's own apps, warm, medians of three boots: launch to the window settled, and the app's memory (PSS). GNOME's photo viewer shows the photo at about 1.4 s; its 3.19 is when its controls hide.</caption><thead><tr><th scope="col"></th><th scope="col">Aizome</th><th scope="col">GNOME</th><th scope="col">KDE Plasma</th></tr></thead><tbody>
<tr><th scope="row">Terminal, files, settings, editor, PDF, calculator, s</th><td>0.11–0.17</td><td>0.47–0.79</td><td>0.27–1.22</td></tr>
<tr><th scope="row">Photo viewer, a 6000 × 3375 JPEG, s</th><td>0.32</td><td>3.19</td><td>0.58</td></tr>
<tr><th scope="row">Those apps' memory, MiB</th><td>15–57</td><td>143–253</td><td>34–177</td></tr>
<tr><th scope="row">The photo viewer's memory, MiB</th><td>326</td><td>436</td><td>166</td></tr>
</tbody></table></div>
<p>Under load GNOME missed 0.8% of its refreshes drawing every frame; Aizome and Plasma missed none. Where Aizome loses: at rest its shell wakes about 47 times a second, more than all of GNOME together, though its processor time at rest is as low as theirs; Hotaru takes more memory than Konsole and Shashin more than Gwenview; and the system it installs, 8.5 GB, is no smaller than GNOME's. In a 2 GiB machine all three boot and log in, each desktop's memory about what it was at 4 GiB. These are a virtual machine's figures, drawn on the processor: on real graphics hardware all three would change. {doc}</p>"#,
        ram = chart("desk-ram", "The desktop's memory over a system with none, MiB (lower is better)", "MiB", 0, &ram,
            "MemTotal − MemAvailable 190 s after login, minus the no-desktop VM's 370 MiB; median of three boots at 4 GiB."),
        key = chart("desk-key", "Keystroke to the frame on screen, with two busy threads, median, frames (lower is better)", "frames", 2, &key,
            "220 presses a boot through a virtual keyboard to a probe window, timed to the presentation at 75 Hz (13.3 ms a frame)."),
        boot = chart("desk-boot", "Boot to a settled login screen, s (lower is better)", "s", 1, &boot,
            "QEMU's start to the last change on screen before three still seconds."),
        login = chart("desk-login", "Login: Enter to a settled desktop, s (lower is better)", "s", 1, &login,
            "The second login of each boot, by password."),
        doc = doc("desktops.md", "Every measurement, the VMs and the method."),
    )
}

fn window_cost() -> String {
    let first = [
        ours("Aizome's kit", 19.0, "range 18.3–19.7 ms; no GPU driver loaded"),
        other("Qt 6 Widgets", 42.8, "range 42.4–46.8 ms; no GPU driver loaded"),
        other("Qt 6 Quick", 86.5, "range 85.4–88.0 ms; GL driver loaded"),
        other("GTK 4", 92.7, "range 90.9–93.8 ms; Vulkan and GL drivers loaded"),
        other("libadwaita", 115.0, "range 114–116 ms; Vulkan and GL drivers loaded"),
    ];
    let private = [
        ours("Aizome's kit", 4.7, "PSS 12.4 MiB, RSS 19.0 MiB, 13 shared objects"),
        other("Qt 6 Widgets", 11.6, "PSS 37.7 MiB, RSS 64.9 MiB, 98 shared objects"),
        other("Qt 6 Quick", 36.8, "PSS 92.4 MiB, RSS 159 MiB, 156 shared objects"),
        other("GTK 4", 16.8, "PSS 40.5 MiB, RSS 88.4 MiB, 131 shared objects"),
        other("libadwaita", 19.4, "PSS 46.1 MiB, RSS 97.4 MiB, 152 shared objects"),
    ];
    format!(
        r#"<p class="headline">The same window, a label and a button, in five toolkits.</p>
<p>Before an app does anything, its toolkit has a price: the libraries it loads, the drivers it opens, the memory it fills. Aizome's kit draws on the processor into memory the compositor shows, so an app loads no GL or Vulkan driver, and the toolkit, the kit and the app are one Rust program with almost nothing to link. GTK 4 and libadwaita are what GNOME's apps are built on; Qt Widgets and Qt Quick are KDE's.</p>
<div class="charts">{first}{private}</div>
<p>The kit's one loss is small: every Aizome app looks at its settings file once a second, so a theme change applies at once without inotify. That is one wakeup a second at rest, against none for GTK and Qt; it costs 0.02 ms of processor time a second.</p>"#,
        first = chart(
            "window-first-frame",
            "Launch to the window on screen, ms (lower is better)",
            "ms",
            1,
            &first,
            "Median of 10 alternating runs, from exec to the compositor mapping the window (its first frame), in a private headless sway on the Radeon. Starting is processor-bound for every toolkit: the time is the CPU spent."
        ),
        private = chart(
            "window-private-memory",
            "Private memory of the open window, MiB (lower is better)",
            "MiB",
            1,
            &private,
            "Anonymous PSS three seconds after the first frame: the memory no other process shares. Shared libraries add more (the table has PSS and RSS), less of it per app when many apps share them."
        ),
    )
}

/// One app in a job: its name, whether it's Aizome's, its median first
/// frame (ms), PSS and private memory (MiB) and wakeups a second at rest.
struct App(&'static str, bool, f64, f64, f64, f64);

/// Each job with the apps that do it, from data/design/{small-apps,
/// document-apps,monitors}.json via summarize.py.
const JOBS: &[(&str, &[App])] = &[
    ("Calculator", &[
        App("Soroban", true, 19.0, 14.3, 5.0, 1.0),
        App("GNOME Calculator", false, 153.0, 61.0, 25.1, 0.0),
        App("KCalc", false, 65.8, 56.0, 14.8, 0.07),
    ]),
    ("Clock", &[
        App("Tokei", true, 19.2, 16.2, 5.5, 5.0),
        App("GNOME Clocks", false, 145.0, 59.3, 24.2, 0.0),
    ]),
    ("Text editor, 2,514 lines", &[
        App("Sumi", true, 19.0, 16.3, 5.6, 8.5),
        App("GNOME Text Editor", false, 158.0, 67.5, 26.5, 0.0),
        App("KWrite", false, 107.0, 77.9, 21.3, 12.0),
    ]),
    ("Images, a 6000 × 4000 JPEG", &[
        App("Shashin", true, 19.0, 117.0, 105.0, 1.0),
        App("Loupe", false, 153.0, 136.0, 28.0, 0.0),
        App("Gwenview", false, 109.0, 192.0, 133.0, 0.07),
    ]),
    ("PDF, 756 pages", &[
        App("Shiori", true, 19.2, 69.5, 55.0, 5.6),
        App("Papers", false, 149.0, 129.0, 87.7, 0.0),
        App("Okular", false, 186.0, 150.0, 86.1, 0.56),
    ]),
    ("Files, a folder of 2,000", &[
        App("Hako", true, 19.4, 36.8, 22.4, 8.6),
        App("Files (Nautilus)", false, 171.0, 97.5, 60.9, 0.0),
        App("Dolphin", false, 114.0, 70.7, 24.2, 0.07),
    ]),
    ("System monitor", &[
        App("Myaku", true, 18.9, 43.7, 30.1, 12.9),
        App("GNOME System Monitor", false, 190.0, 68.1, 32.6, 47.7),
        App("Plasma System Monitor", false, 201.0, 187.0, 89.9, 105.0),
    ]),
    ("Disk usage", &[
        App("Kura", true, 19.2, 15.5, 5.3, 0.98),
        App("Disk Usage Analyzer", false, 159.0, 44.3, 22.7, 5.7),
        App("Filelight", false, 164.0, 129.0, 50.5, 0.0),
    ]),
];

fn apps() -> String {
    let scale = JOBS.iter().flat_map(|(_, a)| a.iter().map(|a| a.2)).fold(0.0, f64::max);
    // One label column for every panel, so the shared scale draws equal values equally.
    let all: Vec<Bar> = JOBS.iter().flat_map(|(_, a)| a.iter().map(|a| Bar { label: a.0, value: 0.0, ours: a.1, note: "" })).collect();
    let label_w = label_width(&all);
    let mut multiples = String::new();
    let mut rows = String::new();
    for (i, (job, apps)) in JOBS.iter().enumerate() {
        let bars: Vec<Bar> = apps
            .iter()
            .map(|a| Bar { label: a.0, value: a.2, ours: a.1, note: "" })
            .collect();
        multiples += &Chart {
            id: &format!("apps-{i}"),
            title: job,
            unit: "ms",
            decimals: 1,
            bars: &bars,
            caption: "",
            scale,
            table: false,
            label_w,
        }
        .html();
        for (j, a) in apps.iter().enumerate() {
            let _ = write!(
                rows,
                r#"<tr{cls}><th scope="row">{job}</th><td class="app-name">{name}</td><td>{ff}</td><td>{pss}</td><td>{private}</td><td>{wake}</td></tr>"#,
                cls = if a.1 { r#" class="ours""# } else { "" },
                job = if j == 0 { esc(job) } else { String::new() },
                name = esc(a.0),
                ff = fmt(a.2),
                pss = fmt(a.3),
                private = fmt(a.4),
                wake = fmt(a.5),
            );
        }
    }
    format!(
        r#"<p class="headline">Aizome's apps are on screen in 19 ms, whatever they open.</p>
<p>Aizome's apps beside the GNOME and KDE apps that do the same job, measured the same way as the windows above, six times each, outside their own desktops in the same neutral compositor. The document apps open the same files: a 6000 × 4000 photo, the 756-page PDF standard, a folder of 2,000 files. GNOME's apps took 145–190 ms to show a window and KDE's 66–201 ms; every Aizome app took 19, because the kit shows the window first and decodes, parses and reads on other threads.</p>
<p class="chart-title">Launch to the window on screen, ms (lower is better), one scale for all</p>
<div class="charts multiples">{multiples}</div>
<div class="table-wrap"><table class="numbers apps-table"><caption>Medians of six runs. PSS counts shared memory once, split between the processes sharing it; private memory is the part no other process shares. Wakeups are context switches a second over 15 s with nothing happening.</caption><thead><tr><th scope="col">Job</th><th scope="col">App</th><th scope="col">First frame, ms</th><th scope="col">PSS, MiB</th><th scope="col">Private, MiB</th><th scope="col">Wakeups a second at rest</th></tr></thead><tbody>{rows}</tbody></table></div>
<p>Memory follows the same pattern, with one exception that is about where the document lives rather than the toolkit: Shashin keeps the decoded photo, 92 MiB of pixels, in its own memory, while Loupe shares it with its sandboxed decoders and hands it to the GPU. By PSS Shashin still uses least. Where Aizome's apps lose is waking at rest: Tokei's clock and Sumi's caret are meant to, but Hako's and Shiori's wakeups are housekeeping on timers, and GNOME's apps woke none at all. The system monitors read the system all the time; Myaku, reading every process each second, spent the most processor time at it, 23 ms a second against GNOME's 18 and Plasma's 13 at their own intervals. {doc}</p>"#,
        doc = doc("design.md#2-whole-apps-beside-gnomes-and-kdes", "The data and method."),
    )
}

/// Three significant figures, as the tables give them.
fn fmt(v: f64) -> String {
    if v == 0.0 {
        "0".into()
    } else if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}").trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn idle() -> String {
    format!(
        r##"<p class="headline">Left alone, the whole desktop drew one frame a minute: the clock.</p>
<p>A headless Aizome session as a login starts it (the compositor, its shell and xwayland-satellite, a fresh user with the defaults), left alone for a minute after settling, three times at 1920 × 1080 and three at 3840 × 2160. The compositor's own statistics count the frames it drew; nothing was drawn but the menu bar's clock turning over.</p>
{tiles}
<p>And in use: runnymede's own session, its compositor driving a 3840 × 2160 display at 240 Hz while this work's agents wrote into a terminal for nearly five hours, drew 146,990 frames: 8.5 a second on average, of the 240 the display could show. Each took about half a millisecond of processor time, 99 in 100 within 1.3 ms.</p>
<p>The shell isn't silent yet: it does a millisecond of work a second but wakes about 53 times, most of them from its system-service watchers checking for a stop request every 250 ms. That, and its memory growing over that long session, are in <a href="#costs">what it costs</a>. {doc}</p>"##,
        tiles = stats(&[
            ("1", "frame drawn in a minute at rest, at 1080p and at 4K"),
            ("0.19 ms", "of the compositor's processor time a second at rest"),
            ("172 MiB", "the whole session's memory (PSS) at 1920 × 1080; 193 MiB at 4K"),
            ("8.5 fps", "average in a working session on a 240 Hz display"),
        ]),
        doc = doc("design.md#3-an-idle-desktop-draws-nothing", "The data and method."),
    )
}

fn compositor() -> String {
    // Median over three rounds of each run's p50 / p99, ms of processor time per frame.
    let rows = [
        ("Idle, two seconds, three windows", "no frames", "no frames", "no frames"),
        ("Moving a window", "0.37 / 0.44", "0.48 / 2.06", "0.24 / 0.29"),
        ("A client drawing every frame", "0.30 / 0.39", "0.35 / 0.42", "0.18 / 0.21"),
        ("Switching Desks, sliding", "0.33 / 1.46", "0.44 / 0.62", "0.18 / 0.37"),
        ("The overview opening and closing", "0.52 / 1.83", "0.96 / 2.92", "1.19 / 2.79"),
        ("A window moving under a frosted menu", "0.54 / 0.66", "0.75 / 2.33", "0.38 / 0.45"),
        ("A full-screen client drawing every frame", "0.25 / 0.34", "0.13 / 0.39", "1.33 / 1.46"),
    ];
    let mut body = String::new();
    for (what, radeon, nvidia, pixman) in rows {
        let _ = write!(body, r#"<tr><th scope="row">{what}</th><td>{radeon}</td><td>{nvidia}</td><td>{pixman}</td></tr>"#);
    }
    format!(
        r#"<p class="headline">At 3840 × 2160, every frame the compositor drew took under 3.2 ms of processor time, and a still screen took none.</p>
<p>The compositor's own benchmark runs a headless compositor at 3840 × 2160, scale 2, opens test windows and plays its scenarios, reading the compositor's statistics for every frame: the processor time from gathering the scene to submitting the frame. It ran three rounds on each renderer: GLES on the Radeon, GLES on the RTX 5090, and pixman, the software fallback.</p>
<div class="table-wrap"><table class="numbers"><caption>Milliseconds of processor time per frame, median and 99th percentile, medians of three rounds.</caption><thead><tr><th scope="col">Scenario</th><th scope="col">GLES, Radeon</th><th scope="col">GLES, RTX 5090</th><th scope="col">pixman (software)</th></tr></thead><tbody>{body}</tbody></table></div>
<p>Only damage is drawn, so moving a window repaints the strips it leaves and enters, and two seconds with nothing changing drew no frames at all. Software composing costs the processor less than GLES does until the whole screen changes; a full-screen client changing every frame costs it 1.3 ms against GLES's 0.1–0.3, which is why the GPU composites and pixman is the fallback. {doc}</p>"#,
        doc = doc("design.md#5-the-compositors-frames-at-4k", "The data and method."),
    )
}

fn scrolling() -> String {
    let cpu = [
        ours("List, pixels moved", 50.0, "range 42.3–50.7 ms a second; the default"),
        other("List, drawn again", 86.3, "range 84.3–91.0; AIZOME_NO_PIXEL_SCROLL=1"),
        ours("Editor, pixels moved", 52.0, "range 49.0–57.3"),
        other("Editor, drawn again", 94.0, "range 84.3–99.3"),
    ];
    format!(
        r#"<p class="headline">Scrolling moves what was drawn and paints only the strip that comes into view: the app does 42–45% less work.</p>
<p>The kit's gallery scrolled its 10,000-row list and table two rows every 20 ms, and its editor a line every 20 ms, for 30 seconds at 2880 × 1800, three times each way: with the pixels moved, as Aizome's apps always scroll, and with the kit made to draw the whole view again.</p>
<div class="charts">{cpu}</div>
<p>Both kept up with every step, 50 frames a second, so the difference is spare time: each frame's drawing took a third less (196 µs against 284 for the list, 209 against 321 for the editor), on a processor that could afford either. On a slower one, or a larger window, that is the difference between keeping up and not. The socket's power sensor, which reads whole watts, could barely tell the two apart. {doc}</p>"#,
        cpu = chart("scroll-cpu", "The gallery's processor time while scrolling, ms a second (lower is better)", "ms/s", 1, &cpu,
            "Medians of three 30-second runs each, alternating, in a private headless sway on the Radeon."),
        doc = doc("design.md#6-scrolling-moves-pixels", "The data and method."),
    )
}

fn hidden() -> String {
    format!(
        r#"<p class="headline">A window that can't be seen isn't asked to draw. On a still desktop, it drew nothing at all.</p>
<p>A GL client that draws as fast as the compositor lets it, 56 frames a second when visible, ran for 20 seconds in each state, three times: visible, with another Desk shown, and minimised. The compositor tells a window to draw after a frame that showed it; a window that isn't shown is told at most once a second, and only when the screen draws a frame anyway.</p>
<div class="table-wrap"><table class="numbers"><caption>The same in all three rounds. Processor time in milliseconds a second.</caption><thead><tr><th scope="col"></th><th scope="col">Visible</th><th scope="col">On another Desk</th><th scope="col">Minimised</th></tr></thead><tbody>
<tr><th scope="row">The client's frames</th><td>56 a second</td><td>none</td><td>none</td></tr>
<tr><th scope="row">The client's processor time</th><td>3.5</td><td>0</td><td>0</td></tr>
<tr><th scope="row">The compositor's processor time</th><td>9.0–9.5</td><td>0</td><td>0–0.5</td></tr>
</tbody></table></div>
<p>Hidden on a desktop where nothing else changed, the window cost nothing, and it drew again the moment its Desk came back. On a busy desktop it would draw at most once a second. The socket's power wandered between 21 and 27 W across these runs, more than a small client costs, so the processor time is the measure. {doc}</p>"#,
        doc = doc("design.md#7-hidden-windows-dont-draw", "The data and method."),
    )
}

fn budget() -> String {
    let rows = [
        ("Toggle a switch", "0.03", "0.06"),
        ("Type a character in a 2,000-line editor", "0.22", "1.75"),
        ("Scroll a 10,000-row list a notch", "0.12", "0.30"),
        ("The menu bar's clock ticking", "0.07", "0.18"),
        ("A window opening and closing (the shell)", "0.24", "1.38"),
        ("A key typed in the palette", "0.40", "0.83"),
        ("Typing in Sumi", "0.38", "0.81"),
        ("Hako: ↓ in a column of 10,000 files", "1.20", "1.37"),
        ("Hako: scrolling that column", "0.28", "0.41"),
        ("Myaku: a new reading of 600 processes", "2.26", "2.69"),
        ("Myaku: clicking a row", "0.19", "4.36"),
    ];
    let mut body = String::new();
    for (what, p50, max) in rows {
        let _ = write!(body, r#"<tr><th scope="row">{}</th><td>{p50}</td><td>{max}</td></tr>"#, esc(what));
    }
    format!(
        r#"<p class="headline">Every interaction the desktop's tests time fits the 4 ms budget, at the median and at the 95th percentile.</p>
<p>Each app's own tests drive it headlessly and time a frame from the event through update, view, layout and rasterising what changed, at scale 2: the processor time the 4 ms budget covers. Release builds, each test alone on the machine.</p>
<div class="table-wrap"><table class="numbers"><caption>Milliseconds of processor time per frame: the median and the slowest frame of each test.</caption><thead><tr><th scope="col">Interaction</th><th scope="col">Median</th><th scope="col">Slowest</th></tr></thead><tbody>{body}</tbody></table></div>
<p>The slowest steady case is Myaku reading 600 processes, 2.3 ms; the slowest single frame, 4.4 ms, was one of 30 clicks on Myaku's process list (the budget is judged at the 95th percentile, and the harness's figures are upper bounds; on the laptop that click's worst was 11 ms). The budget was set on a laptop, an Intel Core Ultra 7 365; on this desktop the same cases ran 1.15 to 1.9 times faster. {doc}</p>"#,
        doc = doc("design.md#8-the-4-ms-budget-on-this-machine", "The data and method."),
    )
}

fn media() -> String {
    let film = [
        ours("Utsushi", 0.20, "VA-API decode, frames to the compositor as dmabufs"),
        other("mpv → dmabuf", 0.42, "--hwdec=vaapi --vo=dmabuf-wayland: set up as Utsushi works"),
        other("mpv → gpu-next", 0.51, "--hwdec=vaapi, its GPU renderer"),
        other("VLC 3", 0.46, "its defaults"),
        other("mpv defaults", 1.36, "decodes in software"),
    ];
    let uhd = [
        ours("Utsushi", 0.89, "no frames dropped"),
        other("mpv → dmabuf", 1.06, "no frames dropped"),
        other("mpv → gpu-next", 7.62, "557 frames dropped"),
        other("VLC 3", 7.60, "decodes AV1 in software; 315 dropped"),
        other("mpv defaults", 13.32, "software; 145 dropped"),
    ];
    format!(
        r#"<p class="headline">Utsushi plays video for as little extra power as any player, and does the least work around each frame.</p>
<p>Utsushi decodes on the GPU and hands each decoded frame to the compositor as it is, never copying it or drawing it again. Against mpv and VLC, each clip played full screen for a minute under Aizome's compositor on the Radeon, three times or more, and the socket's power compared with idle measured between them.</p>
<div class="charts">{film}{uhd}</div>
<p>At 1080p every player that decodes on the GPU sits a few tenths of a watt over idle, within the sensor's noise of each other, and mpv set up the way Utsushi works is within 0.2 W of it on every clip (and below it for AV1). At 4K Utsushi was lowest. The large differences come from decoding in software, as mpv's defaults do (and the <code>mpv.conf</code> Sekka ships), and from a player drawing every 4K frame again on the GPU. Around the frames Utsushi does the least: 0.7–0.9% of a core and about 147 wakeups a second at 1080p, against 1.1–2.0% and 260–450 for the others, with the smallest memory, and its first frame on screen in 58 ms against 76–172. Paused, it wakes four times a second. {doc}</p>"#,
        film = chart("media-1080", "Extra power playing a 1080p H.264 film, W (lower is better)", "W", 2, &film,
            "Socket power over idle, median of three or more one-minute trials; differences within about 0.2 W are noise."),
        uhd = chart("media-4k60", "Extra power playing 4K60 AV1, W (lower is better)", "W", 2, &uhd,
            "The same, for a 3840 × 2160 AV1 clip at 60 frames a second."),
        doc = doc("media.md", "Every clip, player and measurement, and the method."),
    )
}

fn terminals() -> String {
    let start = [
        ours("Hotaru", 49.6, "range 49.2–66.1 ms"),
        other("Ghostty", 178.1, "range 177.7–193.9 ms"),
        other("gnome-terminal", 129.9, "range 129.7–131.0 ms; includes the bus starting its server"),
        other("Konsole", 130.1, "range 129.6–130.2 ms"),
        other("Console", 130.2, "range 130.0–146.3 ms"),
        other("foot", 65.7, "range 49.9–66.3 ms"),
    ];
    let ascii = [
        ours("Hotaru", 1047.0, "64 MiB in 61 ms; 2.0 J over idle"),
        other("Ghostty", 1018.0, "63 ms; 2.5 J"),
        other("gnome-terminal", 407.0, "157 ms; 2.9 J"),
        other("Konsole", 74.0, "859 ms; 12.3 J"),
        other("Console", 418.0, "153 ms; 2.4 J"),
        other("foot", 441.0, "145 ms; 2.9 J"),
    ];
    let cursor = [
        ours("Hotaru", 127.0, "64 MiB in 506 ms; 9.7 J over idle"),
        other("Ghostty", 22.0, "2,868 ms; 36.1 J"),
        other("gnome-terminal", 89.0, "717 ms; 10.3 J"),
        other("Konsole", 90.0, "708 ms; 10.4 J"),
        other("Console", 95.0, "676 ms; 9.5 J"),
        other("foot", 99.0, "647 ms; 10.1 J"),
    ];
    let key = [
        ours("Hotaru", 26.8, "1.61 frames at 60 Hz; p99 35.6 ms"),
        other("Ghostty", 30.0, "1.80 frames; p99 38.0 ms"),
        other("gnome-terminal", 11.6, "0.70 frames; p99 20.2 ms"),
        other("Konsole", 21.2, "1.27 frames; p99 29.9 ms"),
        other("Console", 11.6, "0.70 frames; p99 20.3 ms"),
        other("foot", 11.8, "0.71 frames; p99 20.1 ms"),
    ];
    format!(
        r#"<p class="headline">Hotaru starts first and reads plain output fastest. It shows a keystroke a frame later than the quickest.</p>
<p>Hotaru, Aizome's terminal, against Ghostty, GNOME's two terminals (gnome-terminal and Console), KDE's Konsole and foot, each in its own headless sway on the Radeon at 60 Hz, with the same font at the same size, a 120 × 40 grid, no scrollback and the cursor still. The program inside paints numbered marker rows, and a frame counts only when the compositor's captured image shows the whole update. 132 trials, alternating, every one on a quiet machine.</p>
<div class="charts">{start}{key}{ascii}{cursor}</div>
<p>Hotaru is on screen a frame before foot and 80–128 ms before the rest. It reads 64 MiB of plain text faster than any of them, for the least energy, and cursor-heavy output (the redraws of a full-screen program) 1.3 times as fast as the next and 5.8 times as fast as Ghostty. It loses elsewhere: foot reads styled text faster (431 MiB/s against 348), Ghostty, gnome-terminal and Console read Unicode faster (193–215 against 188), and every keystroke shows about a frame later than in foot or the VTE terminals. Input reaches every terminal's program within a third of a millisecond; Hotaru's extra frame is all between the program's answer and the frame, which fits the toolkit drawing on the compositor's frame callbacks, though these runs don't prove it.</p>
<p>At rest with the cursor still, Hotaru wakes once a second; foot, gnome-terminal and Konsole don't wake at all, and Ghostty wakes 28 times and keeps the GPU busy 4.3 ms a second with nothing changing. Out of the box Hotaru blinks its cursor forever, where GNOME stops after ten seconds. Its memory, 47 MiB, is Console's and two thirds of Konsole's; foot needs 7. And its CPU-drawn buffers cost the compositor more: scrolling a row at 60 Hz took sway 91 ms of processor time per 120 updates with Hotaru, 43 with Ghostty. {doc}</p>"#,
        start = chart("term-start", "Launch to the first frame, ms (lower is better)", "ms", 1, &start,
            "Ten fresh launches each, warm caches; the first captured frame showing the whole first screen."),
        key = chart("term-key", "Keystroke to frame, median ms (lower is better)", "ms", 1, &key,
            "1,000 presses each: a virtual keyboard presses a key, the program answers with a one-cell update, and the time ends at the captured frame showing it."),
        ascii = chart("term-ascii", "Plain text read, MiB/s (higher is better)", "MiB/s", 0, &ascii,
            "64 MiB of ASCII written into the terminal as fast as it reads, four rounds. Energy is the socket's over its idle power."),
        cursor = chart("term-cursor", "Cursor and erase output read, MiB/s (higher is better)", "MiB/s", 0, &cursor,
            "64 MiB of cursor movement, erasing and short writes, as a full-screen program sends them."),
        doc = doc("terminals.md", "Every measurement, the configurations and the method."),
    )
}

fn pdf() -> String {
    let time = [
        ours("Shiori (hayro fork)", 4.17, "0.48 of PDFium's time; faster on 1,045 of 1,179 pages"),
        other("PDFium", 8.70, "nixpkgs' build, 8110, AGG; Chrome's engine"),
        other("hayro upstream", 9.36, "1.08 of PDFium's time"),
        other("MuPDF", 10.98, "1.26 of PDFium's time"),
        other("Poppler, Cairo", 11.37, "1.31; as Papers and Evince draw"),
        other("Poppler, Splash", 21.35, "2.46; as Okular and pdftoppm draw"),
    ];
    let energy = [
        ours("Shiori (hayro fork)", 386.0, "0.46 of PDFium's energy"),
        other("PDFium", 833.0, ""),
        other("hayro upstream", 745.0, "0.89"),
        other("MuPDF", 976.0, "1.17"),
        other("Poppler, Cairo", 1273.0, "1.53"),
        other("Poppler, Splash", 1998.0, "2.40"),
    ];
    format!(
        r#"<p class="headline">Shiori draws PDF pages in half PDFium's time, for half its energy.</p>
<p>Shiori draws with hayro, a PDF renderer written in Rust, through Aizome's fork of it, tuned for speed. Against the engines Linux readers are built on (PDFium, Chrome's; MuPDF; Poppler, under Papers, Evince and Okular) it drew the pdf.js project's test corpus: 1,012 files, mostly bug reports and broken files, the first five pages of each at twice their size, one process per file and one thread each, three timed runs.</p>
<div class="charts">{time}{energy}</div>
<p>Per page, half drew within 1.10 ms (PDFium 1.45) and 99 in 100 within 44 ms (PDFium 81). No hayro build crashed or ran out of time on any file; PDFium, MuPDF and Poppler each crashed on one or two and timed out on up to three. Its pages are as close to PDFium's as MuPDF's and Poppler's are: 61 of 1,189 pages differ on more than 5% of their pixels, against 86 for MuPDF and 124 for Poppler. It is still slower than PDFium on meshes, soft masks over images, nested transparency groups and a few colour set-ups, 1.3–2.4 times PDFium's time on those pages. pdf.js, run in Node, took nine times PDFium's time in rounds of its own.</p>
<p>On conformance there is no suite of right answers for drawing PDFs, only engines' own regression tests. hayro's suite, about 1,560 rendering tests, includes 101 files from PDFium's bug tracker, checked against hayro's own reference images: upstream hayro passes all of them; the fork passes 58 within the suite's three-level tolerance, and its speed changes put the other 43 within anti-aliasing of the references. On PDFium's own pixel tests, which hayro doesn't run, the fork draws 132 of the 182 that apply (73%) within 1% of the pixels PDFium expects; PDFium itself draws all 182. The rest are missing appearance streams, knockout groups, a few shadings and patterns, and malformed files PDFium repairs. {doc}</p>"#,
        time = chart("pdf-time", "Time to draw the same 1,179 pages, s (lower is better)", "s", 2, &time,
            "The pages every engine drew, each page's time the median of three runs, totalled."),
        energy = chart("pdf-energy", "Energy for 5,895 pages, J (lower is better)", "J", 0, &energy,
            "The 969 files every engine finished alike, five times over, at the processor socket; median of three rounds. Starting a process per file costs 67 J of it."),
        doc = doc("pdf.md", "Every measurement, the conformance runs and the method."),
    )
}

/// Where the decisions cost something, from design.md's list.
fn costs() -> String {
    format!(
        r#"<p class="headline">What the design costs, and what isn't done yet.</p>
<ul class="costs">
<li><strong>Not silent at rest yet.</strong> GNOME's apps woke no times a second in these tests; Aizome's woke one to nine times, and the shell about 53. Every app checks its settings file once a second (0.02 ms of processor time), the system-service watchers check for a stop request four times a second each, Shiori's watchdog ticks while no page is drawing, and Sumi's caret, like Hotaru's, blinks without stopping. Each is small; together they keep cores out of their deepest sleep, and each has a known fix.</li>
<li><strong>No shared libraries.</strong> Each app is one Rust program with the toolkit built in, 13–24 MB on disk; two Aizome apps each map their own copy of the toolkit's code (10–13 MiB each), where two GTK apps share one. The apps still start with a third or less of GTK's and Qt's private memory.</li>
<li><strong>Pictures in memory.</strong> Drawing on the processor keeps what the shell draws as pixels in RAM: 39 MiB at 1920 × 1080, 55 MiB at 3840 × 2160. A GPU-drawn shell would hold them in video memory instead.</li>
<li><strong>A long session's shell grew.</strong> Over four unattended hours the live shell's private memory went from 372 MiB to 693 MiB, in its worker threads' allocator arenas; a fresh one holds 55 MiB. The cause isn't established yet.</li>
<li><strong>Copies to the GPU.</strong> The compositor uploads what changed in a CPU-drawn buffer; a GL client's is on the GPU already. Damage tracking keeps the copies small, but a terminal scrolling every frame costs the compositor about twice what Ghostty does.</li>
</ul>
<p>{doc}</p>"#,
        doc = doc("design.md#what-the-design-costs", "The measurements behind each."),
    )
}

pub fn page(_site: &Site) -> Result<Page> {
    let mut b = head(
        "Performance",
        &format!(
            r#"<p class="eyebrow">Performance</p><h1>Fast because it does less.</h1><p class="lead">Aizome draws only what changed, on the processor, into memory the compositor shows. Its apps open no GPU context, its desktop left alone draws a frame a minute, and every frame has a budget of 4 ms. These are the measurements behind that, taken on one desktop on 2 November 2026, against GNOME, KDE Plasma and the programs people use beside Aizome. Each section links to its data and the way to repeat it.</p>{tiles}"#,
            tiles = stats(&[
                ("19 ms", r##"for an Aizome app to show its window; GNOME's and KDE's took 66–201 ms (<a href="#apps">apps</a>)"##),
                ("+221 MiB", r##"the whole desktop's memory in a small VM; GNOME +567, Plasma +875 (<a href="#desktops">desktops</a>)"##),
                ("0.53 frames", r##"keystroke to screen with the processor busy, in that VM; GNOME 0.97, Plasma 1.01"##),
                ("0.48×", r##"of PDFium's time for Shiori's engine to draw the same 1,179 pages, and 0.46× its energy (<a href="#pdf">PDF</a>)"##),
            ]),
        ),
        "",
    );

    b += &win("decisions", "Six decisions", "", DECISIONS);
    b += &win("desktops", "The whole desktop", "", &desktops());
    b += &win("window", "The cost of a window", "", &window_cost());
    b += &win("apps", "Whole apps", "", &apps());
    b += &win("idle", "An idle desktop", "", &idle());
    b += &win("compositor", "The compositor's frames", "", &compositor());
    b += &win("scrolling", "Scrolling", "", &scrolling());
    b += &win("hidden", "Hidden windows", "", &hidden());
    b += &win("budget", "The 4 ms budget", "", &budget());
    b += &win("terminals", "Terminals", "", &terminals());
    b += &win("media", "Media", "", &media());
    b += &win("pdf", "PDF", "", &pdf());
    b += &win("costs", "What it costs", "", &costs());

    b += &win(
        "method",
        "How it was measured",
        "",
        &format!(
            r#"<div class="cols"><p>One machine, runnymede: an AMD Ryzen 7 9800X3D (8 cores, 16 threads) with 62 GiB of RAM, its Radeon graphics and an NVIDIA RTX 5090 driving a 3840 × 2160 display, on Sekka with Linux 7.3. Every program compared comes from Sekka's own Nixpkgs, at the versions a Sekka system installs, with its defaults unless a section says otherwise.</p><p>One measurement at a time: each held a lock that kept the others, and any builds, off the machine. Programs alternate rather than running all of one and then the next, after an unmeasured warm-up, and the figures are medians. Power is the processor socket's own sensor (PPT, cores, Radeon graphics and I/O die), checked against its RAPL energy counter; there is no battery on a desktop.</p></div>
<p>These are measurements of headless compositors and virtual machines, not of light leaving a screen, and they are one machine's. The {readme} says how each was taken, and its data and harness are in the repository beside it.</p>"#,
            readme = doc("README.md", "dataset's README"),
        ),
    );

    Ok(Page {
        path: "/performance/".into(),
        title: "Aizome's performance: measured against GNOME, KDE and the rest".into(),
        description: "Why Aizome is fast and light, with the data: the cost of a window, the desktop against GNOME and KDE Plasma in a small VM, Hotaru against Ghostty and others, Utsushi's energy against mpv and VLC, and Shiori's PDF renderer against PDFium.".into(),
        menu: Menu::Performance,
        body: b,
    })
}
