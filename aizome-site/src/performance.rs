//! /performance/: why Aizome is fast, and how it compares, with the
//! measurements behind it. Every number here comes from docs/performance/
//! (its README, design.md, desktops.md, terminals.md, media.md, pdf.md, the
//! why-*.md analyses, pdf-verdicts.md and their data/), measured on one
//! desktop on 2 November 2026, with idle-update follow-ups on 6–7 November.
//! When those records change, change this with them.

use crate::chart::{Bar, Chart, label_width, other, ours};
use crate::html::{Menu, Page, esc};
use crate::pages::head;
use crate::{Site, win};
use anyhow::Result;
use std::fmt::Write;

/// The repository's docs/performance, where each section's data and the
/// way to repeat it live.
const DATA: &str = "https://github.com/bitemyapp/sekka/blob/main/docs/performance";

/// Ghostty's author's post on the zero-scrollback run, kept with its
/// screenshots in data/terminals/ghostty-author-2026-11-02.
const POST: &str = "https://x.com/mitchellh/status/2117374562333872319";

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
        other("KDE Plasma", 1.01, "13.5 ms, 26.2 at the 99th (1.97 frames); 11.3 ms after each run's first seconds, against Aizome's 7.4"),
    ];
    format!(
        r#"<p class="headline">In a small virtual machine, Aizome boots and logs in faster than GNOME or Plasma, uses much less memory, and gets keystrokes on screen sooner.</p>
<p>We installed the same Sekka system three times, changing only which desktop the installer picked, plus a fourth copy with no desktop at all to show what the base system uses. Each one ran in a virtual machine with 2 CPUs, 4 GiB of RAM and no 3D acceleration, so all three desktops render on the CPU, much like they would in most VMs and cloud desktops. We booted each one three times, taking turns, and timed both the boot and the login. After two minutes at idle, a small probe window measured how long keystrokes took to reach the screen at 75 Hz, with and without two threads keeping the CPU busy. Last, we launched each desktop's own apps, cold and warm.</p>
<div class="charts">{ram}{key}{boot}{login}</div>
<p>Part of the memory gap is services that come with how each desktop is set up rather than with the desktop itself. In Plasma, NixOS's module turns on Akonadi, KDE's mail and calendar store, with its MariaDB database, and the session also starts Baloo, KDE's file indexer: about 230 MiB together. GNOME starts calendar and search services and an input method that a distribution could leave out. Without those, the desktops still need about 255 MiB (GNOME) and 410 MiB (Plasma) more than Aizome, and the largest part of that is the compositor and shell. GNOME's figure also includes about 79 MiB for Sekka's large 4928 × 3264 wallpaper, which GNOME Shell keeps at full size, in RAM here since the VM has no GPU.</p>
<p>A keystroke reached the screen sooner in Aizome because its compositor draws a window's update as soon as it arrives. Mutter, GNOME's compositor, holds each update until a few milliseconds before the next refresh, so under load almost half of the key presses waited a whole refresh longer. Plasma's median includes the first 6 to 15 seconds of every run, when each key press was a refresh late; after that, under load, it took 11.3 ms against Aizome's 7.4.</p>
<div class="table-wrap"><table class="numbers"><caption>Each desktop's own apps, medians of three boots, timed from launch until the window stops changing. Cold is the first launch after boot and warm the next one. About 0.11 s is the fastest this harness can time, and most of Aizome's apps are at that floor. GNOME's photo viewer shows the photo at about 1.4 s, and its time runs to 3.19 s because its on-screen controls fade out then. Memory is PSS on the warm launch.</caption><thead><tr><th scope="col"></th><th scope="col">Aizome</th><th scope="col">GNOME</th><th scope="col">KDE Plasma</th></tr></thead><tbody>
<tr><th scope="row">Terminal, files, settings, editor, PDF and calculator (Plasma has none here), cold, s</th><td>0.11–0.17</td><td>0.47–0.82</td><td>0.27–1.27</td></tr>
<tr><th scope="row">The same, warm, s</th><td>0.11–0.17</td><td>0.47–0.79</td><td>0.27–1.22</td></tr>
<tr><th scope="row">Photo viewer, a 6000 × 3375 JPEG, cold and warm, s</th><td>0.32</td><td>3.19 (photo at 1.4)</td><td>0.58</td></tr>
<tr><th scope="row">Those apps' memory, MiB</th><td>15–57</td><td>143–253</td><td>34–177</td></tr>
<tr><th scope="row">The photo viewer's memory, MiB</th><td>326</td><td>436</td><td>166</td></tr>
</tbody></table></div>
<p>Under load, a window drawing on every frame lost 0.84% of its frames in GNOME and 0.09% in one of Plasma's three runs. Aizome didn't lose any.</p>
<p>Not everything went Aizome's way. In this VM its shell woke about 47 times a second while idle (48–55 in the 7 November headless rerun below), more than all of GNOME put together, though its CPU time at idle was as low as the others'. Hotaru, Aizome's terminal, took more memory than Konsole, KDE's terminal (57 MiB with its fish shell, against 37 MiB with bash). Shashin, Aizome's image viewer, held twice as much memory as Gwenview, KDE's image viewer, and Aizome's installed system (8.5 GB) is no smaller than GNOME's. With only 2 GiB of RAM, all three still boot and log in fine, and each desktop uses about as much memory as it did with 4 GiB. These numbers come from a VM rendering on the CPU, and on real graphics hardware all three desktops would behave differently. {doc} {why}</p>"#,
        ram = chart("desk-ram", "Memory used by the desktop itself, MiB (lower is better)", "MiB", 0, &ram,
            "Memory in use 190 s after login, minus the 370 MiB the system uses with no desktop. Median of three boots at 4 GiB, each desktop as Sekka sets it up."),
        key = chart("desk-key", "Keystroke to screen with the CPU busy, median, in frames (lower is better)", "frames", 2, &key,
            "220 key presses per boot, sent through a virtual keyboard to a probe window and timed until the frame was presented. At 75 Hz, a frame is 13.3 ms."),
        boot = chart("desk-boot", "Boot to a ready login screen, s (lower is better)", "s", 1, &boot,
            "From the moment the VM starts until the screen has stopped changing for three seconds."),
        login = chart("desk-login", "Login, from pressing Enter to a settled desktop, s (lower is better)", "s", 1, &login,
            "A returning user's password login. First-run windows were dismissed once, in a setup boot before the measured ones."),
        doc = doc("desktops.md", "Full results, the VM setup and how it was measured."),
        why = doc("why-desktop.md", "Where each desktop's memory and time go."),
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
        r#"<p class="headline">Aizome's apps put their window on screen in about 19 ms, whatever they're opening. The content follows.</p>
<p>We put Aizome's apps up against the GNOME and KDE apps that do the same jobs, six runs each, all outside their own desktops in the same neutral compositor (sway) on the processor's built-in Radeon graphics. The document apps all opened the same files, a 6000 × 4000 photo, the 756-page PDF specification and a folder of 2,000 files. GNOME's apps took 145–190 ms to show a window, and KDE's took 66–201 ms. Every Aizome app took about 19 ms, because the kit puts the window up first and does the decoding, parsing and reading on other threads. That's the time until the window appears, not until what's in it is ready. Shashin, Aizome's image viewer, is still decoding the photo when its window comes up, and Shiori, Aizome's PDF reader, draws its first page after its window is already showing. For time to a finished window, see the VM results above, where most of Aizome's apps had stopped changing by about 0.11 s and the photo viewer by 0.32 s.</p>
<p class="chart-title">Time from launch until the window appears, ms (lower is better), all on the same scale</p>
<div class="charts multiples">{multiples}</div>
<div class="table-wrap"><table class="numbers apps-table"><caption>2 November 2026, medians of six runs. PSS counts shared memory once and splits it between the processes using it. Private memory is the part no other process shares. Wakeups are context switches per second over 15 seconds with nothing happening.</caption><thead><tr><th scope="col">Job</th><th scope="col">App</th><th scope="col">Window appears, ms</th><th scope="col">PSS, MiB</th><th scope="col">Private, MiB</th><th scope="col">Wakeups a second at rest</th></tr></thead><tbody>{rows}</tbody></table></div>
<p>The biggest single difference is that GTK 4 and Qt Quick start a GPU API before they draw anything, and Aizome's apps never do. After that come each toolkit's own set-up, such as parsing its theme's CSS, and loading and linking 100–200 shared libraries. Two things about this machine and Sekka's setup make the others slower than they'd be elsewhere. GTK's Vulkan loader loads every Vulkan driver installed here (14 of them, NVIDIA's among them) to ask what it offers, about 50 ms of GTK's time. And Sekka's session has every Qt app, KDE's included, load Qt's GTK 3 platform theme, which costs about 23–29 ms and 4.7 MiB at startup. Neither changes the order, but on another machine or setup the gaps would be smaller. Aizome's apps have room to get faster too, since 12 of the kit's 19 ms go to asking about every installed font at startup.</p>
<p>Memory mostly follows the same pattern. The one exception comes down to where the document is kept rather than the toolkit. Shashin holds the decoded photo (92 MiB of pixels) in its own memory, while Loupe, GNOME's image viewer, shares it with its sandboxed decoders and hands it off to the GPU. Counted by PSS, which splits shared memory between the processes using it, Shashin still uses the least. Where Aizome's apps fall behind is waking up while idle. Tokei, the clock, and Sumi, the text editor, are supposed to, since one shows seconds and the other blinks its cursor. But the wakeups in Hako, the file manager, and in Shiori are housekeeping timers, and six of GNOME's eight apps didn't wake at all. System monitors are always reading the system, and Myaku, Aizome's system monitor, checks every process every second. It used the most CPU doing it, about 23 ms per second compared with 18 for GNOME's monitor and 13 for Plasma's. {doc} {why}</p>"#,
        doc = doc("design.md#2-whole-apps-beside-gnomes-and-kdes", "Full results and how they were measured."),
        why = doc("why-apps.md", "Where each toolkit's startup time and memory go."),
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
        r##"<p class="headline">An idle desktop only draws when a clock or meter changes.</p>
<p>On 7 November 2026 we reran the headless session with a fresh user and default settings: 30 seconds to settle, then one minute at idle, three times at each resolution. The compositor drew a median of five frames at 1920 × 1080 and one at 3840 × 2160; the ranges were 1–9 and 1–3.</p>
{tiles}
<p>Removing settings polling and unrelated view rebuilds left idle package power within run-to-run variation: 19.97 → 20.07 W at 1080p and 19.82 → 19.84 W at 4K, compared with the preceding revision. The shell used 0.63–0.69 ms of CPU per second and woke 48–55 times a second; system-service watchers still check for a stop request every 250 ms. {doc}</p>
<p>In the separate 2 November measurement of the machine's working desktop, driving a 3840 × 2160 display at 240 Hz, it drew 146,990 frames in 4.8 hours: 8.5 a second out of a possible 240. Each frame took about half a millisecond of CPU time, and 99% took under 1.3 ms. {live}</p>"##,
        tiles = stats(&[
            ("5 / 1", "frames drawn in a minute at idle, medians at 1080p / 4K"),
            ("0.13 ms", "of compositor CPU time per second at 1080p; 0.12 ms at 4K"),
            ("181 MiB", "for the whole session (PSS) at 1920 × 1080, and 214 MiB at 4K"),
        ]),
        doc = doc("design.md#9-idle-updates-november-2026", "Rerun results and method."),
        live = doc("design.md#4-a-working-session-at-240-hz", "The working session."),
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
        r#"<p class="headline">At 3840 × 2160, no frame took the compositor more than 3.2 ms of CPU time, and a still screen didn't need any frames at all.</p>
<p>The compositor comes with its own benchmark. It starts a headless compositor at 3840 × 2160 with 2× scaling, opens some test windows and runs through a set of scenarios, recording how much CPU time each frame takes from gathering the scene to submitting it. We ran it three times on each renderer, GLES on the Radeon, GLES on the RTX 5090, and pixman, the software fallback.</p>
<div class="table-wrap"><table class="numbers"><caption>CPU time per frame in milliseconds, as median / 99th percentile, taking the median of three rounds. The single slowest frame in any run was 3.2 ms, the overview opening on the RTX 5090.</caption><thead><tr><th scope="col">Scenario</th><th scope="col">GLES, Radeon</th><th scope="col">GLES, RTX 5090</th><th scope="col">pixman (software)</th></tr></thead><tbody>{body}</tbody></table></div>
<p>Because only damaged areas get redrawn, moving a window just repaints the strips it leaves and the strips it moves into, and two seconds with nothing changing produced no frames at all. Software compositing actually uses less CPU than GLES until the whole screen changes. A full-screen client that redraws every frame costs pixman 1.3 ms per frame, compared with 0.1–0.3 ms for GLES, which is why Aizome composites on the GPU and keeps pixman as the fallback. {doc}</p>"#,
        doc = doc("design.md#5-the-compositors-frames-at-4k", "Full results and how they were measured."),
    )
}

fn budget() -> String {
    let rows = [
        ("Toggle a switch", "0.050", "0.050", "0.23", "6 Nov"),
        ("Type a character in a 2,000-line editor", "0.285", "0.450", "2.55", "6 Nov"),
        ("Scroll a 10,000-row list a notch", "0.145", "0.235", "0.69", "6 Nov"),
        ("The menu bar's clock updating", "0.021", "0.040", "0.114", "6 Nov"),
        ("A menu bar meter updating", "0.020", "0.030", "0.06", "6 Nov"),
        ("A window opening and closing (the shell)", "0.295", "0.555", "2.05", "6 Nov"),
        ("A key typed in the palette", "0.455", "0.865", "1.30", "6 Nov"),
        ("Typing in Sumi, the text editor", "0.38", "0.47", "0.81", "2 Nov"),
        ("Hako, the file manager: ↓ in a column of 10,000 files", "1.20", "1.32", "1.37", "2 Nov"),
        ("Hako: scrolling that column", "0.28", "0.29", "0.41", "2 Nov"),
        ("Myaku, the system monitor: a new reading of 600 processes", "2.26", "2.58", "2.69", "2 Nov"),
        ("Myaku: clicking a row", "0.19", "0.25", "4.36", "2 Nov"),
    ];
    let mut body = String::new();
    for (what, p50, p95, max, date) in rows {
        let _ = write!(body, r#"<tr><th scope="row">{}</th><td>{p50}</td><td>{p95}</td><td>{max}</td><td>{date}</td></tr>"#, esc(what));
    }
    format!(
        r#"<p class="headline">Everything we timed fits within the 4 ms budget, at the median and at the 95th percentile.</p>
<p>Each app's tests time its CPU-side frame work from the input event through updating, layout and drawing the damage, offscreen at 2× scale, in release builds. The 6 November reruns took turns across eight paired runs; unrelated background jobs were active. The clock row now exercises the real clock-update message.</p>
<div class="table-wrap"><table class="numbers"><caption>Elapsed frame time in milliseconds, measured in November 2026. For reruns, Median and 95th are the medians of the eight runs' percentiles; Slowest is the maximum across them.</caption><thead><tr><th scope="col">Interaction</th><th scope="col">Median</th><th scope="col">95th</th><th scope="col">Slowest</th><th scope="col">Date</th></tr></thead><tbody>{body}</tbody></table></div>
<p>The slowest regular case remains Myaku reading 600 processes, at 2.3 ms (2.6 ms at the 95th percentile). Its single slowest row click took 4.4 ms. The budget is judged at the 95th percentile. {doc}</p>"#,
        doc = doc("design.md#9-idle-updates-november-2026", "Rerun data and the original frame tests."),
    )
}

fn updates() -> String {
    format!(
        r#"<p>6–7 November 2026: settings now wait for file changes, and clock and meter updates rebuild only the views that use them.</p>
<div class="table-wrap"><table class="numbers"><caption>Before → after, against the preceding code revision. Latency and wakeups measured 6 November; energy measured 7 November after background jobs stopped.</caption><thead><tr><th scope="col">Measurement</th><th scope="col">Before</th><th scope="col">After</th><th scope="col">Change</th></tr></thead><tbody>
<tr><th scope="row">Idle kit window, wakeups/s</th><td>1</td><td>0</td><td>−100%</td></tr>
<tr><th scope="row">Meter frame, median, ms</th><td>0.055</td><td>0.020</td><td>−64%</td></tr>
<tr><th scope="row">Clock frame, median, ms</th><td>0.049</td><td>0.021</td><td>−57%</td></tr>
<tr><th scope="row">Settings save → notification, median, ms</th><td>447</td><td>3.17</td><td>−99.3%</td></tr>
<tr><th scope="row">100,000 meter updates, energy above background, J</th><td>43.46</td><td>20.17</td><td>−53.6%</td></tr>
</tbody></table></div>
<p>All six paired energy trials improved (48–59%). This is CPU-package and integrated-GPU energy for the update workload; idle power showed no detectable saving. {doc}</p>"#,
        doc = doc("design.md#9-idle-updates-november-2026", "Data and method."),
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
        r#"<p class="headline">Utsushi, Aizome's media player, is among the lowest-power players we tried, the lowest at 4K, and does the least work around each frame.</p>
<p>Utsushi decodes video on the GPU and hands each frame straight to the compositor, without copying it or drawing it a second time. We played the same clips for a minute at a time in Aizome's Utsushi, in mpv, a popular open-source player, and in VLC, the cross-platform media player, under Aizome's compositor on the Radeon. Each player ran at least three times, and we compared the CPU socket's power draw with idle readings taken in between. Every player ran full screen except mpv with its dmabuf output, whose full-screen option didn't take under Aizome, so it played in a window the size of the picture. A later run put it full screen, and its work was the same.</p>
<div class="charts">{film}{uhd}</div>
<p>At 1080p, every player that decodes on the GPU sits just a few tenths of a watt above idle, close enough that differences under about 0.2 W are within the noise. The mpv player, set up to work the way Utsushi does, stays within about 0.2 W of it on every clip, and came in lower on a 1080p AV1 clip. At 4K, Utsushi used the least. The big differences come from decoding in software, which mpv does by default (as does the <code>mpv.conf</code> Sekka ships), and from players that redraw every 4K frame on the GPU. Utsushi also does the least work around each frame. At 1080p it uses 0.7–0.9% of a core and wakes about 147 times a second, compared with 1.1–2.0% and 260–450 wakeups for mpv and the VLC player when they decode on the GPU. It uses the least memory too, gets its first frame on screen in 58 ms (the others took 76–172 ms), and wakes just four times a second while paused. {doc}</p>"#,
        film = chart("media-1080", "Extra power while playing a 1080p H.264 film, W (lower is better)", "W", 2, &film,
            "Power above idle for the CPU socket, median of three or more one-minute runs. Differences under about 0.2 W are within the noise."),
        uhd = chart("media-4k60", "Extra power while playing 4K60 AV1, W (lower is better)", "W", 2, &uhd,
            "The same measurement for a 3840 × 2160 AV1 clip at 60 frames per second."),
        doc = doc("media.md", "Every clip, player and result, and how they were measured."),
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
    let key = [
        ours("Hotaru", 26.8, "1.61 frames at 60 Hz; p99 35.6 ms"),
        other("Ghostty", 30.0, "1.80 frames; p99 38.0 ms"),
        other("gnome-terminal", 11.6, "0.70 frames; p99 20.2 ms"),
        other("Konsole", 21.2, "1.27 frames; p99 29.9 ms"),
        other("Console", 11.6, "0.70 frames; p99 20.3 ms"),
        other("foot", 11.8, "0.71 frames; p99 20.1 ms"),
    ];
    // Throughput with history in every terminal: 50 MB for Hotaru and
    // Ghostty, 1,000,000 lines for the rest (six rounds each).
    let ascii = [
        ours("Hotaru", 705.0, "64 MiB in 91 ms; 2.8 J over idle"),
        other("Ghostty 1.4.0", 720.0, "89 ms; 3.4 J"),
        other("Ghostty main", 794.0, "81 ms; 2.9 J; built from 0b1729e"),
        other("gnome-terminal", 230.0, "279 ms; 4.3 J"),
        other("Konsole", 3.3, "19.5 s; 376 J"),
        other("Console", 232.0, "276 ms; 4.2 J"),
        other("foot", 146.0, "439 ms; 5.4 J"),
    ];
    let styled = [
        ours("Hotaru", 301.0, "213 ms; 4.8 J over idle"),
        other("Ghostty 1.4.0", 271.0, "236 ms; 5.6 J"),
        other("Ghostty main", 346.0, "185 ms; 5.6 J"),
        other("gnome-terminal", 156.0, "411 ms; 6.2 J"),
        other("Konsole", 0.7, "95 s; 2,000 J"),
        other("Console", 163.0, "393 ms; 6.2 J"),
        other("foot", 334.0, "191 ms; 3.7 J"),
    ];
    let unicode = [
        ours("Hotaru", 165.0, "389 ms; 7.4 J over idle"),
        other("Ghostty 1.4.0", 72.0, "893 ms; 16.8 J"),
        other("Ghostty main", 251.0, "255 ms; 7.0 J"),
        other("gnome-terminal", 88.0, "728 ms; 11.1 J"),
        other("Konsole", 0.6, "115 s; 2,112 J"),
        other("Console", 88.0, "725 ms; 10.0 J"),
        other("foot", 151.0, "423 ms; 6.2 J"),
    ];
    let cursor = [
        ours("Hotaru", 126.0, "507 ms; 9.6 J over idle"),
        other("Ghostty 1.4.0", 22.0, "2,886 ms; 36.7 J"),
        other("Ghostty main", 181.0, "353 ms; 8.9 J"),
        other("gnome-terminal", 25.0, "2,538 ms; 30.7 J"),
        other("Konsole", 90.0, "708 ms; 10.3 J"),
        other("Console", 24.0, "2,620 ms; 27.0 J"),
        other("foot", 104.0, "618 ms; 8.9 J"),
    ];
    format!(
        r#"<p class="headline">Hotaru, Aizome's terminal, starts faster than any of them. The latest Ghostty, a GPU-rendered terminal, reads output faster, and Hotaru shows keystrokes a frame later than the quickest terminals.</p>
<p>We compared Hotaru, Aizome's terminal emulator, with Ghostty, a fast GPU-rendered terminal, GNOME's two terminals (the older gnome-terminal and the newer Console), KDE's terminal Konsole, and foot, a minimal Wayland terminal. Each one ran in its own headless sway session on the Radeon at 60 Hz, using the same font and size, a 120 × 40 grid and a cursor that doesn't blink. The program running inside each terminal draws numbered marker rows, and a frame only counts once the captured screen shows the complete update. Every trial started on an idle machine, with the terminals taking turns.</p>
<div class="charts">{start}{key}</div>
<p>Hotaru gets on screen one frame ahead of foot, the minimal terminal, and 80–128 ms ahead of everything else. It loses on typing latency. It shows each keystroke about a frame later than foot or GNOME's two terminals, which are both built on GNOME's VTE terminal widget. Input reaches the program in every terminal within a third of a millisecond, so Hotaru's extra frame is spent between the program's response and the frame on screen. Shorter follow-up runs put it in Hotaru's own path from the program's output to its frame, not in the toolkit: a plain text view built on the same toolkit shows a keystroke at the next frame, as GTK and Qt do.</p>
<p>For throughput, we first ran every terminal with no scrollback, as an earlier comparison of Hotaru's core with libghostty-vt, Ghostty's terminal library, had. Mitchell Hashimoto, Ghostty's author, reran that comparison and <a href="{post}">pointed out</a> that with no scrollback libghostty-vt loses a lot of its optimizations, which is fair for the alternate screen that full-screen programs use but not for ordinary terminal use. He also found that Hotaru gets slower as its history grows, which a run without history hides. So that run favoured Hotaru twice over, and we measured everything again with history turned on. Ghostty kept its default of 50 MB and Hotaru got the same 50 MB. The other four only let you set history in lines, so each got 1,000,000 lines, the same as Hotaru's row limit and at least as much as Ghostty's 50 MB holds. We included both the Ghostty release Sekka ships (1.4.0) and Ghostty's main branch as of 2 November (<code>0b1729e</code>). These numbers are for each terminal app as a whole, running on screen, so reading the output, parsing it, keeping history and drawing frames all count. They aren't the speed of a VT parser on its own, and can't be compared directly with benchmarks of a terminal library such as libghostty-vt.</p>
<div class="charts">{ascii}{styled}{unicode}{cursor}</div>
<p>With history, Ghostty's main branch is the fastest on all four kinds of output, and Hotaru manages 66–89% of its speed. Hotaru comes next on Unicode and cursor-heavy output, and third on plain and styled text, behind the two Ghostty builds and behind main and foot respectively. Against the 1.4.0 release, Hotaru is about even on plain text, a little ahead on styled text, and well ahead on Unicode and cursor-heavy output, both of which Ghostty has sped up since that release. History hit the others harder. GNOME's gnome-terminal and Console fell to about 230 MiB/s on plain text and 25 on cursor-heavy output, and foot to 146 on plain text. Konsole, KDE's terminal, keeps its 1,000,000 lines in a fixed-size history. It spent the whole flood busy on the CPU and managed 3.3 MiB/s or less on everything but cursor-heavy output. A million lines is far more history than most people keep (Konsole's default is 1,000), so the table below also has the first run, where no terminal kept any history.</p>
<p>How much memory a full history takes depends on where each terminal keeps it. The VTE-based terminals write theirs to compressed files on disk and held 38 and 69 MiB. Hotaru (94 MiB) and Ghostty (109 and 120 MiB) keep their 50 MB in memory, partly compressed, and Konsole and foot held 942 MiB and 1.5 GiB for their million uncompressed lines. Hotaru's own biggest drop was on plain text, from 1,047 MiB/s with no scrollback to 705 MiB/s with history.</p>
<div class="table-wrap"><table class="numbers"><caption>The first run, with no scrollback in any terminal: MiB/s, medians of four rounds. Ghostty main wasn't in this run. Ghostty's author counts a run without scrollback fair for the alternate screen, which keeps no history, but for ordinary output it favours Hotaru over Ghostty. It's also here for the other terminals, whose million lines of history above are far more than they keep by default.</caption><thead><tr><th scope="col">No scrollback, MiB/s</th><th scope="col">Hotaru</th><th scope="col">Ghostty 1.4.0</th><th scope="col">gnome-terminal</th><th scope="col">Konsole</th><th scope="col">Console</th><th scope="col">foot</th></tr></thead><tbody>
<tr><th scope="row">Plain text</th><td>1,047</td><td>1,018</td><td>407</td><td>74</td><td>418</td><td>441</td></tr>
<tr><th scope="row">Styled text</th><td>348</td><td>258</td><td>216</td><td>86</td><td>256</td><td>431</td></tr>
<tr><th scope="row">Unicode text</th><td>188</td><td>215</td><td>198</td><td>93</td><td>193</td><td>171</td></tr>
<tr><th scope="row">Cursor-heavy output</th><td>127</td><td>22</td><td>89</td><td>90</td><td>95</td><td>99</td></tr>
</tbody></table></div>
<p>The graphs in <a href="{post}">Ghostty's author's post</a> show different gaps, and on plain text the opposite order. Two things account for most of it. He timed the terminal libraries alone, fed bytes with nothing drawn, where these runs time each app on screen. And Hotaru's feed-only mode keeps no history, so in his runs only Ghostty kept any, while here both kept 50 MB. Plain text is the output history slows Hotaru most, so with history on both sides Ghostty main is ahead on it. The builds are close: our Ghostty main is three commits after his, with one more fix for erasing blank rows, and our Hotaru includes a later change to how it reads from the program. Why Ghostty's lead on styled text is smaller here and on Unicode larger, we haven't measured. Drawing is the likeliest difference.</p>
<div class="table-wrap"><table class="numbers"><caption>Ghostty main's speed as a multiple of Hotaru's. His run timed the VT libraries alone, with 50 MB of history for Ghostty and none for Hotaru. These runs timed whole terminals on screen, both with 50 MB.</caption><thead><tr><th scope="col">Ghostty main ÷ Hotaru</th><th scope="col">His library-level run</th><th scope="col">These app runs</th></tr></thead><tbody>
<tr><th scope="row">Plain text</th><td>0.78×</td><td>1.13×</td></tr>
<tr><th scope="row">Styled text</th><td>1.35×</td><td>1.15×</td></tr>
<tr><th scope="row">Unicode text</th><td>1.35×</td><td>1.52×</td></tr>
<tr><th scope="row">Cursor-heavy output</th><td>1.67×</td><td>1.44×</td></tr>
<tr><th scope="row">Hyperlinks</th><td>1.17×</td><td>not measured</td></tr>
</tbody></table></div>
<p>In the 2 November runs, sitting idle with a steady cursor, Hotaru woke once a second. foot, gnome-terminal and Konsole don't wake at all, while Ghostty wakes 28 times a second and keeps the GPU busy for 4.3 ms of every second even though nothing changes. With default settings, Hotaru's cursor blinks forever, whereas GNOME's terminals stop blinking after ten seconds. At startup Hotaru uses 47 MiB of memory, about the same as Console and two thirds of what Konsole uses here, where Konsole runs outside Plasma and its libraries count in full. foot needs just 7. Hotaru's CPU-drawn buffers also cost the compositor more. Scrolling a row at 60 Hz cost sway 0.76 ms of CPU time per update with Hotaru, against 0.36 ms with Ghostty. {doc}</p>"#,
        start = chart("term-start", "Launch to first frame, ms (lower is better)", "ms", 1, &start,
            "Ten fresh launches each, with warm caches, timed until the first captured frame that shows the whole first screen."),
        key = chart("term-key", "Keystroke to screen, median, ms (lower is better)", "ms", 1, &key,
            "1,000 presses each. A virtual keyboard presses a key, the program responds by updating one cell, and the clock stops at the captured frame that shows the change."),
        ascii = chart("term-ascii", "Plain text with history, MiB/s (higher is better)", "MiB/s", 1, &ascii,
            "64 MiB of ASCII text written into the terminal as fast as it can read it, six rounds each. History is 50 MB for Hotaru and Ghostty and 1,000,000 lines for the others. Energy is measured at the CPU socket, above idle."),
        styled = chart("term-styled", "Styled text with history, MiB/s (higher is better)", "MiB/s", 1, &styled,
            "64 MiB of coloured, bold and underlined log lines."),
        unicode = chart("term-unicode", "Unicode text with history, MiB/s (higher is better)", "MiB/s", 1, &unicode,
            "64 MiB of CJK, accented letters and emoji."),
        cursor = chart("term-cursor", "Cursor-heavy output with history, MiB/s (higher is better)", "MiB/s", 1, &cursor,
            "64 MiB of cursor movement, erasing and short writes, like what a full-screen program sends."),
        doc = doc("terminals.md", "Full results, the exact settings and how they were measured."),
        post = POST,
    )
}

fn pdf() -> String {
    let time = [
        ours("Shiori (hayro fork)", 4.17, "0.48 of PDFium's time; faster on 1,045 of 1,179 pages"),
        other("PDFium", 8.70, "nixpkgs' build, 8110, AGG; the engine in Chrome"),
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
    // Each engine's total over the pages every engine drew, as a multiple
    // of the fork's: summary.json's common_set and realworld-summary.json's
    // totals.
    let multiples = [
        ("PDFium", "2.1×", "1.3×"),
        ("hayro upstream", "2.2×", "1.2×"),
        ("MuPDF", "2.6×", "1.4×"),
        ("Poppler, Cairo", "2.7×", "2.1×"),
        ("Poppler, Splash", "5.1×", "5.9×"),
    ];
    let mut rows = String::new();
    for (engine, corpus, real) in multiples {
        let _ = write!(rows, r#"<tr><th scope="row">{engine}</th><td>{corpus}</td><td>{real}</td></tr>"#);
    }
    format!(
        r#"<p class="headline">Shiori's renderer draws the pages of a test corpus in about half the time PDFium, the PDF engine in Chrome, takes, and the pages of real documents in about three quarters of it.</p>
<p>Shiori, Aizome's PDF reader, renders pages with hayro, an open-source PDF renderer written in Rust, using Aizome's own fork of it, tuned for speed. We compared that renderer with the engines most Linux PDF readers use, which are PDFium (the engine in Chrome, here as Nixpkgs builds it), MuPDF (the library behind readers such as Zathura) and Poppler (the library behind GNOME's Papers and Evince and KDE's Okular). The test set was the 1,012 files in the test corpus of pdf.js, Firefox's built-in PDF renderer, many of them bug reports and deliberately broken files. Each engine drew the first five pages of every file at 2× size, with one process per file and a single thread, over three timed runs.</p>
<div class="charts">{time}{energy}</div>
<p>The pdf.js corpus is mostly small and broken files, so we also timed seven real documents, public domain or CC-BY: US government publications such as the Constitution, IRS tax documents and the federal budget, and a scientific paper. Each engine drew the first 20 pages of each (all of the two shorter ones), five times. On those, the fork's lead is smaller. It took 0.37 s for the 113 pages against PDFium's 0.48 and was the faster on 102 of them. It was the fastest of the six engines on four of the seven documents, MuPDF on two, and PDFium on the scientific paper, by under a millisecond.</p>
<div class="table-wrap"><table class="numbers"><caption>Each engine's drawing time as a multiple of Shiori's renderer's, over the pages every engine drew. Higher means slower than Shiori's renderer.</caption><thead><tr><th scope="col">Engine</th><th scope="col">pdf.js corpus, 1,179 pages</th><th scope="col">Real documents, 113 pages</th></tr></thead><tbody>{rows}</tbody></table></div>
<p>On the 1,179 corpus pages, the fork's median page took 1.10 ms and its 99th percentile 37.7 ms, against 1.45 and 79.6 ms for PDFium. Neither hayro build, Aizome's fork or upstream, crashed or timed out on any file, while PDFium, MuPDF and Poppler each crashed on one or two files and timed out on as many as three. The fork's output is about as close to PDFium's as MuPDF's and Poppler's are. Of the 1,189 pages the fork and PDFium both drew at the same size (a slightly different set from the 1,179 that every engine drew), only 61 differ from PDFium on more than 5% of their pixels, compared with 86 for MuPDF and 124 for Poppler. It's still slower than PDFium on some kinds of page, such as meshes, soft masks over images, nested transparency groups and some shadings, taking 1.3 to 2.4 times as long on those.</p>
<p>There's no official answer key for how a PDF should render, only each engine's own regression tests. hayro's test suite has about 1,560 rendering tests, including 101 files taken from PDFium's bug tracker, and those are checked against hayro's own reference images. Upstream hayro passes all 101. The fork passes 58 within the suite's tolerance of three colour levels, and its speed optimisations leave the other 43 differing only by anti-aliasing. hayro doesn't run PDFium's own pixel tests, so we ran those too. The fork renders 132 of the 182 tests that apply (73%) within 1% of PDFium's expected images, and PDFium itself matches 180 exactly and all 182 within 1%. We judged each of the fork's 50 misses against the PDF specification. PDFium is the more correct in 19 and the fork in 6. In 24 both are acceptable, mostly damaged files the specification gives no answer for, and in 1 both are wrong. Where the fork is wrong, it's mostly in recovering damaged files, plus a few missing features such as generating an appearance for annotations and form fields that don't include one. {doc} {verdicts}</p>"#,
        time = chart("pdf-time", "Time to draw the same 1,179 pages of the pdf.js corpus, s (lower is better)", "s", 2, &time,
            "Only pages every engine managed to draw, using each page's median time over three runs, added up."),
        energy = chart("pdf-energy", "Energy to draw 5,895 pages, J (lower is better)", "J", 0, &energy,
            "The 969 files every engine handled the same way, five times over, measured at the CPU socket. Median of three rounds. Just starting a process for each file accounts for 67 J of this."),
        doc = doc("pdf.md", "Full results, the conformance runs and how they were measured."),
        verdicts = doc("pdf-verdicts.md", "Each miss, judged against the specification."),
    )
}

/// Where the design costs something, from design.md's list.
fn costs() -> String {
    format!(
        r##"<p class="headline">What the design costs, and what isn't finished yet.</p>
<ul class="costs">
<li><strong>Not quiet enough at idle.</strong> In the 2 November app comparison, six of GNOME's eight apps did not wake at all. Aizome's settings polling has since been removed: the idle kit window now records zero wakeups. The shell still wakes 48–55 times a second. Its system-service watchers check for a stop request four times a second, Shiori's watchdog keeps ticking while no page is being drawn, and the blinking cursors in Sumi and Hotaru never stop.</li>
<li><strong>No shared libraries.</strong> Each app is a single Rust program with the toolkit compiled in, 13–24 MB on disk. Two Aizome apps running side by side each load their own copy of the toolkit code (10–13 MiB each), where two GTK apps would share one.</li>
<li><strong>Pictures stay in RAM.</strong> Because the shell draws on the CPU, its bar, Deskbar and desktop are pictures in system memory, so a bigger screen costs RAM: at 3840 × 2160 the shell held 16 MiB more private memory than at 1920 × 1080. Its wallpaper is an 8 MiB picture, but it costs the shell about 24 MiB.</li>
<li><strong>The shell grew over a long session.</strong> Over four unattended hours, the live shell's private memory went from 372 MiB to 693 MiB, and it kept growing more slowly, to about 785 MiB nine hours later. The growth is in its worker threads' allocator arenas, and a freshly started shell uses 55 MiB. We haven't tracked down the cause yet.</li>
<li><strong>Copying to the GPU.</strong> For CPU-drawn windows, the compositor has to upload whatever changed to the GPU, while a GL client's buffer is already there. Damage tracking keeps those uploads small, but a terminal scrolling every frame still costs the compositor about twice as much as Ghostty does.</li>
<li><strong>Terminal latency and throughput.</strong> Hotaru shows a keystroke about a frame later than foot or GNOME's terminals, and the latest Ghostty reads output faster (<a href="#terminals">terminals</a>).</li>
</ul>
<p>{doc}</p>"##,
        doc = doc("design.md#what-the-design-costs", "The measurements behind each of these."),
    )
}

pub fn page(_site: &Site) -> Result<Page> {
    let mut b = head(
        "Performance",
        &format!(
            r#"<p class="eyebrow">Performance</p><h1>Fast because it does less.</h1><p class="lead">Aizome only redraws what has changed, and its apps draw on the CPU into memory they share with the compositor, without starting OpenGL or Vulkan. An idle desktop draws only a few frames a minute, and every part of the desktop has a budget of 4 ms to turn an event into a frame. This page has the measurements behind all that, taken on one desktop on 2 November 2026, with idle and update measurements refreshed on 6–7 November, comparing Aizome with GNOME, KDE Plasma and other programs people often use alongside it, including where Aizome loses. Each section links to its full write-up, with the raw data and how to repeat the test.</p>{tiles}"#,
            tiles = stats(&[
                ("19 ms", r##"for an Aizome app's window to appear, with its content following, where GNOME's and KDE's apps took 66–201 ms as Sekka sets them up (<a href="#apps">apps</a>)"##),
                ("+221 MiB", r##"of RAM for the whole desktop in a small VM, against +567 for GNOME and +875 for Plasma as Sekka sets them up (<a href="#desktops">desktops</a>)"##),
                ("1–5 frames", r##"drawn in a minute by an idle desktop, medians at 4K and 1080p (<a href="#idle">idle</a>)"##),
                ("0.48×", r##"PDFium's time for Shiori's renderer to draw the same 1,179 test pages, and 0.78× on real documents (<a href="#pdf">PDF</a>)"##),
            ]),
        ),
        "",
    );

    b += &win("desktops", "The whole desktop", "", &desktops());
    b += &win("apps", "Apps", "", &apps());
    b += &win("idle", "An idle desktop", "", &idle());
    b += &win("compositor", "The compositor's frames", "", &compositor());
    b += &win("budget", "The 4 ms budget", "", &budget());
    b += &win("updates", "Less work per update", "", &updates());
    b += &win("terminals", "Terminals", "", &terminals());
    b += &win("media", "Media", "", &media());
    b += &win("pdf", "PDF", "", &pdf());
    b += &win("costs", "What it costs", "", &costs());

    b += &win(
        "method",
        "How it was measured",
        "",
        &format!(
            r#"<div class="cols"><p>Everything was measured on one machine, an AMD Ryzen 7 9800X3D (8 cores, 16 threads) with 62 GiB of RAM, its built-in Radeon graphics, and an NVIDIA RTX 5090 driving a 3840 × 2160 display, running Sekka on Linux 7.3. Every program we compared came from Sekka's own Nixpkgs, at the same versions a Sekka install would get, except Ghostty's main branch, which we built. Each ran with Sekka's default settings unless a section says otherwise.</p><p>Measurements ran one at a time under a benchmark lock; the 6 November latency rerun had unrelated background jobs, which were stopped for the 7 November energy measurements. Programs took turns, with warm-up or settling before measurement, and the figures shown are medians. A desktop has no battery to measure, so power came from the CPU socket's own sensor (PPT, which covers the cores, the Radeon graphics and the I/O die), cross-checked against its RAPL energy counter. The 7 November idle runs read RAPL only at the interval boundaries, without adding a polling loop.</p></div>
<p>These are measurements of headless compositors and virtual machines rather than of light coming off a real screen, and they all come from a single machine. The {readme} explains how each one was taken, and the data and test harnesses sit next to it in the repository.</p>
<details class="chart-table"><summary>Versions of everything measured</summary><div class="table-wrap"><table class="numbers versions"><thead><tr><th scope="col">Software</th><th scope="col">Version</th></tr></thead><tbody>
<tr><th scope="row">Aizome (compositor, shell and apps, including Hotaru, Utsushi and Shiori)</th><td>0.1.0, Sekka d7ecbac (2 Nov); idle/update reruns 2711942e → 553dc6af (6–7 Nov)</td></tr>
<tr><th scope="row">Shiori's renderer, hayro (Aizome's fork), and upstream hayro</th><td>a2e9e62b, 04a801de</td></tr>
<tr><th scope="row">Sekka's Nixpkgs, where everything else comes from</th><td>26.11 (1c89083)</td></tr>
<tr><th scope="row">Linux, systemd</th><td>7.3.0 (2 Nov), 7.3.1 (6–7 Nov); systemd 262</td></tr>
<tr><th scope="row">Mesa (Radeon), NVIDIA driver</th><td>26.2.6, 615.78.08</td></tr>
<tr><th scope="row">GNOME Shell, Mutter, GDM</th><td>50.4, 50.5, 50.2</td></tr>
<tr><th scope="row">GTK 4, libadwaita, GTK 3 (gnome-terminal), VTE</th><td>4.22.4, 1.9.3, 3.24, 0.84</td></tr>
<tr><th scope="row">GNOME's apps (Calculator, Clocks, Loupe, Papers, Files, System Monitor, Disk Usage Analyzer, Console)</th><td>50.0–50.2.2</td></tr>
<tr><th scope="row">GNOME Text Editor, gnome-terminal</th><td>50.1, 3.60.0</td></tr>
<tr><th scope="row">KDE Plasma (KWin, Plasma Login), Plasma System Monitor</th><td>6.8.2</td></tr>
<tr><th scope="row">KDE Frameworks, Qt</th><td>6.31.0, 6.12.0</td></tr>
<tr><th scope="row">KDE's apps (Konsole, Dolphin, KCalc, KWrite, Gwenview, Okular, Filelight)</th><td>26.08.2</td></tr>
<tr><th scope="row">Ghostty (release, main)</th><td>1.4.0, 1.4.1-dev 0b1729e</td></tr>
<tr><th scope="row">foot</th><td>1.28.0</td></tr>
<tr><th scope="row">sway (the neutral compositor), wlroots</th><td>1.12, 0.20.2</td></tr>
<tr><th scope="row">mpv, libplacebo</th><td>0.41.0, 7.360.1</td></tr>
<tr><th scope="row">VLC</th><td>3.0.24</td></tr>
<tr><th scope="row">FFmpeg (Utsushi and mpv; VLC)</th><td>9.0.2; 8.1.3</td></tr>
<tr><th scope="row">PDFium (GCC build, AGG renderer)</th><td>8110</td></tr>
<tr><th scope="row">MuPDF</th><td>1.28.5</td></tr>
<tr><th scope="row">Poppler, Cairo</th><td>26.10.0, 1.18.4</td></tr>
<tr><th scope="row">QEMU (the VMs)</th><td>11.1.2</td></tr>
</tbody></table></div></details>"#,
            readme = doc("README.md", "dataset's README"),
        ),
    );

    Ok(Page {
        path: "/performance/".into(),
        title: "Aizome's performance: measured against GNOME, KDE and the rest".into(),
        description: "Why Aizome is fast and light, with the data: the desktop against GNOME and KDE Plasma in a small VM, Aizome's apps against GNOME's and KDE's, Hotaru against Ghostty and others, Utsushi's energy against mpv and VLC, and Shiori's PDF renderer against PDFium.".into(),
        menu: Menu::Performance,
        body: b,
    })
}
