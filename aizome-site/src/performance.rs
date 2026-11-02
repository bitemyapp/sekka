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
const DECISIONS: &str = r##"<p class="headline">Most of it comes down to six decisions, made early in the toolkit and compositor that everything else is built on.</p>
<ol class="decisions">
<li><strong>One toolkit, drawn on the CPU.</strong> The shell and every app are Rust programs built on the same toolkit (Sukumo, with Aizome's kit on top). Each window has its own render thread that draws into shared memory, and the compositor shows that memory directly. Apps never open a GPU context or load a GL or Vulkan driver, and because the toolkit is compiled into each app, there's very little to load at startup. See <a href="#window">the cost of a window</a> and <a href="#apps">whole apps</a>.</li>
<li><strong>Only redraw what changed.</strong> When an app's state changes, it repaints just the parts of the window that moved or look different. Scrolling shifts the pixels that are already there and only draws the strip that comes into view. The compositor tracks damage the same way, so moving a window doesn't redraw what's inside it, and a screen where nothing changed doesn't render a frame at all. See <a href="#scrolling">scrolling</a> and <a href="#idle">the idle desktop</a>.</li>
<li><strong>Surfaces say what they cover.</strong> Opaque regions tell the compositor what it can skip drawing underneath. Scaling sticks to whole numbers (1×, 2×, 3×), so a window's pixels map one-to-one onto the screen's and never need resampling. See <a href="#compositor">the compositor's frames</a>.</li>
<li><strong>A 4 ms budget.</strong> Every part of the desktop gets 4 ms of CPU time to turn an event into a frame. Each app has tests that check this, and any case that misses is written up. See <a href="#budget">the budget on this machine</a>.</li>
<li><strong>Don't draw what nobody can see.</strong> A window on another Desk, or minimised, is asked to draw at most once a second, and only if the screen is being redrawn anyway. An app that would happily draw flat out ends up drawing almost nothing while it's hidden. See <a href="#hidden">hidden windows</a>.</li>
<li><strong>Use the GPU where it actually helps.</strong> The compositor composites on the GPU and falls back to software on its own if it has to. Video goes straight from the hardware decoder to the compositor as GPU buffers, without passing through the player's memory or a GL context of its own. PDF pages are drawn in a separate worker process for each document. See <a href="#media">media</a> and <a href="#pdf">PDF</a>.</li>
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
        r#"<p class="headline">In a small virtual machine, Aizome boots and logs in faster than GNOME or Plasma, uses a fraction of their memory, and gets keystrokes on screen about half a frame sooner.</p>
<p>We installed the same Sekka system three times, changing only which desktop the installer picked, plus a fourth copy with no desktop at all to show what the base system uses. Each one ran in a virtual machine with 2 CPUs, 4 GiB of RAM and no 3D acceleration, so all three desktops render on the CPU, much like they would in most VMs and cloud desktops. We booted each one three times, taking turns, and timed both the boot and the login. After two minutes at idle, a small probe window measured how long keystrokes took to reach the screen at 75 Hz, with and without two threads keeping the CPU busy. Last, we launched each desktop's own apps, cold and warm.</p>
<div class="charts">{ram}{key}{boot}{login}</div>
<div class="table-wrap"><table class="numbers"><caption>Each desktop's own apps, launched warm (medians of three boots). Times run from launch until the window stops changing. GNOME's photo viewer actually shows the photo at about 1.4 s, and reaches 3.19 s when its on-screen controls fade out.</caption><thead><tr><th scope="col"></th><th scope="col">Aizome</th><th scope="col">GNOME</th><th scope="col">KDE Plasma</th></tr></thead><tbody>
<tr><th scope="row">Terminal, files, settings, editor, PDF, calculator, s</th><td>0.11–0.17</td><td>0.47–0.79</td><td>0.27–1.22</td></tr>
<tr><th scope="row">Photo viewer, a 6000 × 3375 JPEG, s</th><td>0.32</td><td>3.19</td><td>0.58</td></tr>
<tr><th scope="row">Those apps' memory, MiB</th><td>15–57</td><td>143–253</td><td>34–177</td></tr>
<tr><th scope="row">The photo viewer's memory, MiB</th><td>326</td><td>436</td><td>166</td></tr>
</tbody></table></div>
<p>Under load, GNOME missed 0.8% of its frames, while Aizome and Plasma didn't miss any. Aizome doesn't win everything, though. Its shell wakes up about 47 times a second while idle, more than all of GNOME put together, even if the CPU time involved is just as small. Hotaru uses more memory than Konsole, Shashin uses more than Gwenview, and Aizome's installed system (8.5 GB) is no smaller than GNOME's. With only 2 GiB of RAM, all three still boot and log in fine, and each desktop uses about as much memory as it did with 4 GiB. Keep in mind that these numbers come from a VM rendering on the CPU. On real graphics hardware, all three desktops would behave differently. {doc}</p>"#,
        ram = chart("desk-ram", "Memory used by the desktop itself, MiB (lower is better)", "MiB", 0, &ram,
            "Memory in use 190 s after login, minus the 370 MiB the system uses with no desktop. Median of three boots at 4 GiB."),
        key = chart("desk-key", "Keystroke to screen with the CPU busy, median, in frames (lower is better)", "frames", 2, &key,
            "220 key presses per boot, sent through a virtual keyboard to a probe window and timed until the frame was presented. At 75 Hz, a frame is 13.3 ms."),
        boot = chart("desk-boot", "Boot to a ready login screen, s (lower is better)", "s", 1, &boot,
            "From the moment the VM starts until the screen has stopped changing for three seconds."),
        login = chart("desk-login", "Login, from pressing Enter to a settled desktop, s (lower is better)", "s", 1, &login,
            "The second password login of each boot."),
        doc = doc("desktops.md", "Full results, the VM setup and how it was measured."),
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
        r#"<p class="headline">The same tiny window, with a label and a button, built with five different toolkits.</p>
<p>Every toolkit costs something before the app itself does any work, in the libraries it loads, the drivers it opens and the memory it sets up. Aizome's kit draws on the CPU into memory the compositor reads directly, so its apps never load a GL or Vulkan driver. The toolkit, the kit and the app are compiled into a single Rust program, which leaves very little to link at startup. For comparison, GNOME's apps are built on GTK 4 and libadwaita, and KDE's on Qt Widgets and Qt Quick.</p>
<div class="charts">{first}{private}</div>
<p>The kit does pay one small cost here. Every Aizome app checks its settings file once a second, so a theme change applies right away without needing inotify. That's one wakeup per second at idle where GTK and Qt have none, and it uses about 0.02 ms of CPU time per second.</p>"#,
        first = chart(
            "window-first-frame",
            "Time from launch until the window appears, ms (lower is better)",
            "ms",
            1,
            &first,
            "Median of 10 runs, taking turns, from launch until the compositor maps the window. Measured in a private headless sway session on the Radeon. Startup is CPU-bound for every toolkit, so this is essentially CPU time."
        ),
        private = chart(
            "window-private-memory",
            "Private memory once the window is open, MiB (lower is better)",
            "MiB",
            1,
            &private,
            "Private memory three seconds after the window appears, meaning memory no other process shares. Shared libraries add more on top (the table has PSS and RSS), though less per app when several apps share them."
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
        r#"<p class="headline">Aizome's apps show their window in about 19 ms, no matter what they're opening.</p>
<p>Next, we put Aizome's apps up against the GNOME and KDE apps that do the same jobs. They were measured the same way as the windows above, six runs each, all outside their own desktops in the same neutral compositor. The document apps all opened the same files, a 6000 × 4000 photo, the 756-page PDF specification and a folder of 2,000 files. GNOME's apps took 145–190 ms to show a window, and KDE's took 66–201 ms. Every Aizome app took about 19 ms, because the kit puts the window up first and does the decoding, parsing and reading on other threads.</p>
<p class="chart-title">Time from launch until the window appears, ms (lower is better), all on the same scale</p>
<div class="charts multiples">{multiples}</div>
<div class="table-wrap"><table class="numbers apps-table"><caption>Medians of six runs. PSS counts shared memory once and splits it between the processes using it. Private memory is the part no other process shares. Wakeups are context switches per second over 15 seconds with nothing happening.</caption><thead><tr><th scope="col">Job</th><th scope="col">App</th><th scope="col">First frame, ms</th><th scope="col">PSS, MiB</th><th scope="col">Private, MiB</th><th scope="col">Wakeups a second at rest</th></tr></thead><tbody>{rows}</tbody></table></div>
<p>Memory mostly follows the same pattern. The one exception comes down to where the document is kept rather than the toolkit. Shashin holds the decoded photo (92 MiB of pixels) in its own memory, while Loupe shares it with its sandboxed decoders and hands it off to the GPU. Counting shared memory fairly, by PSS, Shashin still uses the least. Where Aizome's apps fall behind is waking up while idle. Tokei's clock and Sumi's blinking cursor are supposed to, but Hako's and Shiori's wakeups are housekeeping timers, and GNOME's apps didn't wake at all. System monitors are always reading the system, and Myaku, which checks every process every second, used the most CPU doing it, about 23 ms per second compared with 18 for GNOME's monitor and 13 for Plasma's. {doc}</p>"#,
        doc = doc("design.md#2-whole-apps-beside-gnomes-and-kdes", "Full results and how they were measured."),
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
        r##"<p class="headline">Left alone, the whole desktop drew one frame a minute, and that was the clock ticking over.</p>
<p>We started a headless Aizome session the way a login would (the compositor, its shell and xwayland-satellite, with a fresh user and default settings), let it settle, and then left it alone for a minute. We did that three times at 1920 × 1080 and three times at 3840 × 2160. According to the compositor's own statistics, the only thing it drew was the menu bar clock changing.</p>
{tiles}
<p>We also looked at a real session, the test machine's own desktop, driving a 3840 × 2160 display at 240 Hz through almost five hours of terminal-heavy work. It drew 146,990 frames, which works out to 8.5 per second out of a possible 240. Each frame took about half a millisecond of CPU time, and 99% took under 1.3 ms.</p>
<p>The shell still isn't completely quiet. It only does about a millisecond of work per second, but it wakes up around 53 times a second, mostly because its system-service watchers check for a stop request every 250 ms. That, and the way its memory grew over that long session, are covered under <a href="#costs">what it costs</a>. {doc}</p>"##,
        tiles = stats(&[
            ("1", "frame drawn per minute at idle, at both 1080p and 4K"),
            ("0.19 ms", "of compositor CPU time per second at idle"),
            ("172 MiB", "for the whole session (PSS) at 1920 × 1080, and 193 MiB at 4K"),
            ("8.5 fps", "on average during a real working session on a 240 Hz display"),
        ]),
        doc = doc("design.md#3-an-idle-desktop-draws-nothing", "Full results and how they were measured."),
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
<div class="table-wrap"><table class="numbers"><caption>CPU time per frame in milliseconds, as median / 99th percentile, taking the median of three rounds.</caption><thead><tr><th scope="col">Scenario</th><th scope="col">GLES, Radeon</th><th scope="col">GLES, RTX 5090</th><th scope="col">pixman (software)</th></tr></thead><tbody>{body}</tbody></table></div>
<p>Because only damaged areas get redrawn, moving a window just repaints the strips it leaves and the strips it moves into, and two seconds with nothing changing produced no frames at all. Software compositing actually uses less CPU than GLES until the whole screen changes. A full-screen client that redraws every frame costs pixman 1.3 ms per frame, compared with 0.1–0.3 ms for GLES, which is why Aizome composites on the GPU and keeps pixman as the fallback. {doc}</p>"#,
        doc = doc("design.md#5-the-compositors-frames-at-4k", "Full results and how they were measured."),
    )
}

fn scrolling() -> String {
    let cpu = [
        ours("List, pixels moved", 50.0, "range 42.3–50.7 ms per second; this is the default"),
        other("List, drawn again", 86.3, "range 84.3–91.0; AIZOME_NO_PIXEL_SCROLL=1"),
        ours("Editor, pixels moved", 52.0, "range 49.0–57.3"),
        other("Editor, drawn again", 94.0, "range 84.3–99.3"),
    ];
    format!(
        r#"<p class="headline">Scrolling reuses what's already been drawn, and that cuts the app's CPU use by more than 40%.</p>
<p>We had the kit's gallery app scroll its 10,000-row list and table two rows every 20 ms, and its editor one line every 20 ms, for 30 seconds at 2880 × 1800. Each test ran three times with pixel moving on, which is how Aizome's apps always scroll, and three times with the kit forced to redraw the whole view instead.</p>
<div class="charts">{cpu}</div>
<p>Both versions kept up at 50 frames per second, so the savings show up as spare time. Each frame took about a third less time to draw (196 µs instead of 284 for the list, and 209 instead of 321 for the editor). This CPU can afford either, but on a slower machine or a bigger window, that margin is what keeps scrolling smooth. The power sensor only reads in whole watts, and it could barely tell the two apart. {doc}</p>"#,
        cpu = chart("scroll-cpu", "Gallery CPU time while scrolling, ms per second (lower is better)", "ms/s", 1, &cpu,
            "Medians of three 30-second runs each, taking turns, in a private headless sway session on the Radeon."),
        doc = doc("design.md#6-scrolling-moves-pixels", "Full results and how they were measured."),
    )
}

fn hidden() -> String {
    format!(
        r#"<p class="headline">Windows you can't see don't get asked to draw. On a quiet desktop, a hidden window drew nothing at all.</p>
<p>We ran a GL demo that draws as fast as the compositor allows, which works out to 56 frames per second while it's visible. It ran for 20 seconds in each of three states (visible, with a different Desk showing, and minimised), and we repeated the whole thing three times. The compositor asks a window to draw after each frame that shows it. A window that isn't showing gets asked at most once a second, and only if the screen is being redrawn for some other reason.</p>
<div class="table-wrap"><table class="numbers"><caption>The results were the same in all three rounds. CPU time is in milliseconds per second.</caption><thead><tr><th scope="col"></th><th scope="col">Visible</th><th scope="col">On another Desk</th><th scope="col">Minimised</th></tr></thead><tbody>
<tr><th scope="row">Frames the demo drew</th><td>56 a second</td><td>none</td><td>none</td></tr>
<tr><th scope="row">The demo's CPU time</th><td>3.5</td><td>0</td><td>0</td></tr>
<tr><th scope="row">The compositor's CPU time</th><td>9.0–9.5</td><td>0</td><td>0–0.5</td></tr>
</tbody></table></div>
<p>With nothing else changing on the desktop, the hidden window cost nothing at all, and it started drawing again as soon as its Desk came back. On a busier desktop it would draw at most once a second. Power readings drifted between 21 and 27 W during these runs, which is more than a small demo like this could account for, so CPU time is the better measure here. {doc}</p>"#,
        doc = doc("design.md#7-hidden-windows-dont-draw", "Full results and how they were measured."),
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
        r#"<p class="headline">Everything we timed fits within the 4 ms budget, at the median and at the 95th percentile.</p>
<p>Each app has its own tests that run it headlessly and time a frame all the way from the input event through updating, layout and drawing whatever changed, at 2× scale. That's the CPU time the 4 ms budget is meant to cover. We used release builds and ran each test alone on the machine.</p>
<div class="table-wrap"><table class="numbers"><caption>CPU time per frame in milliseconds, showing the median and the slowest frame for each test.</caption><thead><tr><th scope="col">Interaction</th><th scope="col">Median</th><th scope="col">Slowest</th></tr></thead><tbody>{body}</tbody></table></div>
<p>The slowest regular case is Myaku reading 600 processes, at 2.3 ms. The single slowest frame was one of 30 clicks on Myaku's process list, which took 4.4 ms. The budget is judged at the 95th percentile and these test timings are upper bounds, and on the laptop where the budget was set, that same click's worst case was 11 ms. That laptop has an Intel Core Ultra 7 365, and this desktop ran the same tests 1.15 to 1.9 times faster. {doc}</p>"#,
        doc = doc("design.md#8-the-4-ms-budget-on-this-machine", "Full results and how they were measured."),
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
        r#"<p class="headline">Utsushi plays video using as little extra power as any player we tried, and less CPU than all of them.</p>
<p>Utsushi decodes video on the GPU and hands each frame straight to the compositor, without copying it or drawing it a second time. We played the same clips full screen for a minute at a time in Utsushi, mpv and VLC under Aizome's compositor on the Radeon, at least three times per player, and compared the CPU socket's power draw with idle readings taken in between.</p>
<div class="charts">{film}{uhd}</div>
<p>At 1080p, every player that decodes on the GPU sits just a few tenths of a watt above idle, close enough that the sensor can't really separate them. mpv configured the way Utsushi works stays within 0.2 W of it on every clip, and came in slightly lower for AV1. At 4K, Utsushi used the least. The big differences come from decoding in software, which mpv does by default (as does the <code>mpv.conf</code> Sekka ships), and from players that redraw every 4K frame on the GPU. Utsushi also does the least work around each frame. At 1080p it uses 0.7–0.9% of a core and wakes about 147 times a second, compared with 1.1–2.0% and 260–450 wakeups for the others. It uses the least memory too, gets its first frame on screen in 58 ms (the others took 76–172 ms), and wakes just four times a second while paused. {doc}</p>"#,
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
    // Throughput with Ghostty's default 50 MB of history in both (six rounds).
    let ascii = [
        ours("Hotaru", 705.0, "64 MiB in 91 ms; 2.8 J over idle"),
        other("Ghostty 1.4.0", 720.0, "89 ms; 3.4 J"),
        other("Ghostty main", 794.0, "81 ms; 2.9 J; built from 0b1729e"),
    ];
    let styled = [
        ours("Hotaru", 301.0, "213 ms; 4.8 J over idle"),
        other("Ghostty 1.4.0", 271.0, "236 ms; 5.6 J"),
        other("Ghostty main", 346.0, "185 ms; 5.6 J"),
    ];
    let unicode = [
        ours("Hotaru", 165.0, "389 ms; 7.4 J over idle"),
        other("Ghostty 1.4.0", 72.0, "893 ms; 16.8 J"),
        other("Ghostty main", 251.0, "255 ms; 7.0 J"),
    ];
    let cursor = [
        ours("Hotaru", 126.0, "507 ms; 9.6 J over idle"),
        other("Ghostty 1.4.0", 22.0, "2,886 ms; 36.7 J"),
        other("Ghostty main", 181.0, "353 ms; 8.9 J"),
    ];
    format!(
        r#"<p class="headline">Hotaru starts faster than any of them, but Ghostty's current code reads output faster, and Hotaru shows keystrokes a frame later than the quickest terminals.</p>
<p>We compared Hotaru, Aizome's terminal, with Ghostty, GNOME's two terminals (gnome-terminal and Console), KDE's Konsole, and foot. Each one ran in its own headless sway session on the Radeon at 60 Hz, using the same font and size, a 120 × 40 grid and a cursor that doesn't blink. The program running inside each terminal draws numbered marker rows, and a frame only counts once the captured screen shows the complete update. Every trial started on an idle machine, with the terminals taking turns.</p>
<div class="charts">{start}{key}</div>
<p>Hotaru gets on screen one frame ahead of foot and 80–128 ms ahead of everything else. It does lose on typing latency, though. It shows each keystroke about a frame later than foot or the VTE-based terminals. Input reaches the program in every terminal within a third of a millisecond, so Hotaru's extra frame is spent between the program's response and the frame on screen. That fits with the toolkit waiting for the compositor's frame callbacks before it draws, but these tests don't prove it.</p>
<p>For throughput, we first ran every terminal with no scrollback. Mitchell Hashimoto, Ghostty's author, pointed out that this switches off much of Ghostty's fast path for ordinary output, and that Hotaru slows down as its history fills up, so that run flattered Hotaru twice over. We measured Hotaru and Ghostty again with Ghostty's default 50 MB of history in both, against the Ghostty release Sekka ships (1.4.0) and Ghostty's main branch as of 2 November (<code>0b1729e</code>). The other four terminals weren't run again, so they aren't in these charts.</p>
<div class="charts">{ascii}{styled}{unicode}{cursor}</div>
<p>With history, Ghostty's main branch is the fastest on all four kinds of output, and Hotaru manages 66–89% of its speed, falling furthest behind on Unicode. Against the 1.4.0 release, Hotaru is about even on plain text, a little ahead on styled text, and well ahead on Unicode and cursor-heavy output, both of which Ghostty has sped up since that release. Hotaru also holds the least memory once its history is full, 94 MiB against 109 MiB for Ghostty 1.4.0 and 120 MiB for main. History cost Hotaru the most on plain text, which it read at 1,047 MiB/s with no scrollback and 705 MiB/s with 50 MB.</p>
<p>Sitting idle with a steady cursor, Hotaru wakes once a second. foot, gnome-terminal and Konsole don't wake at all, while Ghostty wakes 28 times a second and keeps the GPU busy for 4.3 ms of every second even though nothing changes. With default settings, Hotaru's cursor blinks forever, whereas GNOME's terminals stop blinking after ten seconds. At startup Hotaru uses 47 MiB of memory, about the same as Console and two thirds of what Konsole uses, while foot needs just 7. Its CPU-drawn buffers also cost the compositor more. Scrolling a row at 60 Hz took sway 91 ms of CPU time per 120 updates with Hotaru, compared with 43 ms with Ghostty. {doc}</p>"#,
        start = chart("term-start", "Launch to first frame, ms (lower is better)", "ms", 1, &start,
            "Ten fresh launches each, with warm caches, timed until the first captured frame that shows the whole first screen."),
        key = chart("term-key", "Keystroke to screen, median, ms (lower is better)", "ms", 1, &key,
            "1,000 presses each. A virtual keyboard presses a key, the program responds by updating one cell, and the clock stops at the captured frame that shows the change."),
        ascii = chart("term-ascii", "Plain text with 50 MB of history, MiB/s (higher is better)", "MiB/s", 0, &ascii,
            "64 MiB of ASCII text written into the terminal as fast as it can read it, over six rounds. Energy is measured at the CPU socket, above idle."),
        styled = chart("term-styled", "Styled text with 50 MB of history, MiB/s (higher is better)", "MiB/s", 0, &styled,
            "64 MiB of coloured, bold and underlined log lines."),
        unicode = chart("term-unicode", "Unicode text with 50 MB of history, MiB/s (higher is better)", "MiB/s", 0, &unicode,
            "64 MiB of CJK, accented letters and emoji."),
        cursor = chart("term-cursor", "Cursor-heavy output with 50 MB of history, MiB/s (higher is better)", "MiB/s", 0, &cursor,
            "64 MiB of cursor movement, erasing and short writes, like what a full-screen program sends."),
        doc = doc("terminals.md", "Full results, the exact settings and how they were measured."),
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
        r#"<p class="headline">Shiori's renderer draws pages in about half the time PDFium takes, using about half the energy.</p>
<p>Shiori renders PDFs with hayro, a renderer written in Rust, using Aizome's own fork that has been tuned for speed. We compared it with the engines most Linux PDF readers use, PDFium (Chrome's engine), MuPDF, and Poppler (behind Papers, Evince and Okular). The test set was the pdf.js project's corpus of 1,012 files, most of them bug reports and deliberately broken files. Each engine drew the first five pages of every file at 2× size, with one process per file and a single thread, over three timed runs.</p>
<div class="charts">{time}{energy}</div>
<p>Half of the pages took under 1.10 ms each (1.45 ms for PDFium), and 99% took under 44 ms (81 ms for PDFium). Neither hayro build crashed or timed out on any file, while PDFium, MuPDF and Poppler each crashed on one or two files and timed out on as many as three. The fork's output is about as close to PDFium's as MuPDF's and Poppler's are. Only 61 of 1,189 pages differ from PDFium on more than 5% of their pixels, compared with 86 for MuPDF and 124 for Poppler. It's still slower than PDFium on meshes, soft masks over images, nested transparency groups and a few colour setups, taking 1.3–2.4 times as long on those pages. pdf.js, run under Node in separate rounds, took about nine times as long as PDFium.</p>
<p>There's no official answer key for how a PDF should render, only each engine's own regression tests. hayro's test suite has about 1,560 rendering tests, including 101 files taken from PDFium's bug tracker, and those are checked against hayro's own reference images. Upstream hayro passes all 101. The fork passes 58 within the suite's tolerance of three colour levels, and its speed optimisations leave the other 43 differing only by anti-aliasing. hayro doesn't run PDFium's own pixel tests, so we ran those too. The fork renders 132 of the 182 tests that apply (73%) within 1% of PDFium's expected images, and PDFium itself passes all 182. The misses come from missing appearance streams, knockout groups, a few shadings and patterns, and broken files that PDFium manages to repair. {doc}</p>"#,
        time = chart("pdf-time", "Time to draw the same 1,179 pages, s (lower is better)", "s", 2, &time,
            "Only pages every engine managed to draw, using each page's median time over three runs, added up."),
        energy = chart("pdf-energy", "Energy to draw 5,895 pages, J (lower is better)", "J", 0, &energy,
            "The 969 files every engine handled the same way, five times over, measured at the CPU socket. Median of three rounds. Just starting a process for each file accounts for 67 J of this."),
        doc = doc("pdf.md", "Full results, the conformance runs and how they were measured."),
    )
}

/// Where the decisions cost something, from design.md's list.
fn costs() -> String {
    format!(
        r#"<p class="headline">What the design costs, and what isn't finished yet.</p>
<ul class="costs">
<li><strong>Not quiet enough at idle.</strong> In these tests, GNOME's apps didn't wake up at all while idle. Aizome's woke between one and nine times a second, and the shell about 53 times. Every app checks its settings file once a second (about 0.02 ms of CPU each time), the system-service watchers each check for a stop request four times a second, Shiori's watchdog keeps ticking even when no page is being drawn, and the cursors in Sumi and Hotaru blink forever. None of these costs much on its own, but together they keep CPU cores from reaching their deepest sleep states. Each one has a known fix.</li>
<li><strong>No shared libraries.</strong> Each app is a single Rust program with the toolkit compiled in, 13–24 MB on disk. Two Aizome apps running side by side each load their own copy of the toolkit code (10–13 MiB each), where two GTK apps would share one. Even so, Aizome's apps start with a third or less of the private memory that GTK and Qt apps use.</li>
<li><strong>Pictures stay in RAM.</strong> Because the shell draws on the CPU, what it draws is kept as pixels in system memory, 39 MiB at 1920 × 1080 and 55 MiB at 3840 × 2160. A shell drawn on the GPU would keep the same images in video memory instead.</li>
<li><strong>The shell grew over a long session.</strong> Over four unattended hours, the live shell's private memory went from 372 MiB to 693 MiB, and it reached 767 MiB over the next eight and a half hours. The growth is in its worker threads' allocator arenas, and a freshly started shell uses 55 MiB. We haven't tracked down the cause yet.</li>
<li><strong>Copying to the GPU.</strong> For CPU-drawn windows, the compositor has to upload whatever changed to the GPU, while a GL client's buffer is already there. Damage tracking keeps those uploads small, but a terminal scrolling every frame still costs the compositor about twice as much as Ghostty does.</li>
</ul>
<p>{doc}</p>"#,
        doc = doc("design.md#what-the-design-costs", "The measurements behind each of these."),
    )
}

pub fn page(_site: &Site) -> Result<Page> {
    let mut b = head(
        "Performance",
        &format!(
            r#"<p class="eyebrow">Performance</p><h1>Fast because it does less.</h1><p class="lead">Aizome only redraws what has changed, draws it on the CPU, and hands it straight to the compositor. Its apps never open a GPU context, an idle desktop draws one frame a minute, and every part of the desktop has 4 ms to turn an event into a frame. This page has the measurements behind all that, taken on 2 November 2026 on a single desktop machine, comparing Aizome with GNOME, KDE Plasma and other programs people often use alongside it. Each section links to its raw data and explains how to repeat the test.</p>{tiles}"#,
            tiles = stats(&[
                ("19 ms", r##"for an Aizome app to show its window, where GNOME and KDE apps took 66–201 ms (<a href="#apps">apps</a>)"##),
                ("+221 MiB", r##"of RAM for the whole desktop in a small VM, against +567 for GNOME and +875 for Plasma (<a href="#desktops">desktops</a>)"##),
                ("0.53 frames", r##"from keystroke to screen with the CPU busy, in the same VM, where GNOME took 0.97 and Plasma 1.01"##),
                ("0.48×", r##"the time PDFium needs to draw the same 1,179 pages, using 0.46× the energy (<a href="#pdf">PDF</a>)"##),
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
            r#"<div class="cols"><p>Everything was measured on one machine, an AMD Ryzen 7 9800X3D (8 cores, 16 threads) with 62 GiB of RAM, its built-in Radeon graphics, and an NVIDIA RTX 5090 driving a 3840 × 2160 display, running Sekka on Linux 7.3. Every program we compared came from Sekka's own Nixpkgs, at the same versions a Sekka install would get, with default settings unless a section says otherwise.</p><p>Only one measurement ran at a time, and each one held a lock that kept other tests and builds off the machine. Programs took turns rather than running all of one and then all of the next, each started with an unmeasured warm-up run, and the figures shown are medians. A desktop has no battery to measure, so power came from the CPU socket's own sensor (PPT, which covers the cores, the Radeon graphics and the I/O die), cross-checked against its RAPL energy counter.</p></div>
<p>These are measurements of headless compositors and virtual machines rather than of light coming off a real screen, and they all come from a single machine. The {readme} explains how each one was taken, and the data and test harnesses sit next to it in the repository.</p>"#,
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
