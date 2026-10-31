//! What the site says. Every claim here comes from the repository's README,
//! DEVELOPERS.md, component READMEs or docs/screenshots.md; keep it so.

use crate::Site;
use crate::html::{AIZOME, NIGHTLY, Page, REPO, esc};
use crate::tokens::GROUNDS;
use anyhow::Result;
use std::fmt::Write;

/// Aizome's apps as its README and docs/apps.md name them: the name, its
/// kanji, the generic name the Toolchest shows beside it, and what it does.
const APPS: [(&str, &str, &str, &str); 18] = [
    ("Settei", "設定", "Settings", "Every setting, laid out as a Mac's System Settings, and every one searchable."),
    ("Yukimi", "雪見", "Software", "Install, update, go back and free up space, without editing Nix."),
    ("Hako", "箱", "Files", "A file manager in NeXT's spirit: Miller columns, and Quick Look on Space."),
    ("Sumi", "墨", "Text Editor", "Markdown styled as you type, and syntax colouring for code."),
    ("Fusen", "付箋", "Notes", "Plain Markdown notes in ~/Notes; any one can float as a sticky."),
    ("Koyomi", "暦", "Calendar", "Month, week and day, kept as .ics files, with alerts."),
    ("Tokei", "時計", "Clock", "World clocks, alarms that ring with the app closed, a timer and a stopwatch."),
    ("Soroban", "算盤", "Calculator", "Basic, scientific and programmer modes, with a paper tape."),
    ("Tenki", "天気", "Weather", "Now, the next 48 hours and ten days ahead, from Open-Meteo."),
    ("Tansu", "箪笥", "Disks", "Partitions as drawers: every change queued and listed before it runs."),
    ("Kura", "蔵", "Disk Usage", "A sunburst of what fills a disk, the Nix store included."),
    ("Myaku", "脈", "Activity", "Each app with every process it started, together."),
    ("Nisshi", "日誌", "Logs", "The journal and /var/log, filtered as you type."),
    ("Shashin", "写真", "Images", "PNG, JPEG, animated GIF, WebP, TIFF, SVG and more."),
    ("Shiori", "栞", "Documents", "A PDF reader, drawn by hayro, in Rust."),
    ("Utsushi", "映し", "Media Player", "Films, clips and music, with as little work as the machine can do them with."),
    ("Koe", "声", "Dictation", "Speak, and the words are typed into the app in front, recognised on this computer."),
    ("Hotaru", "蛍", "Terminal", "Aizome's own terminal: tabs, split panes and search through scrollback."),
];

pub fn home(site: &Site) -> Result<Page> {
    let mut b = String::new();
    write!(
        b,
        r#"<section class="hero">
  <div class="win focus hero-text">
    {lockup}
    <h1>NixOS you can look after without a text editor.</h1>
    <p class="lead">Sekka is NixOS with a desktop of its own. You live in Aizome, written in Rust from the compositor up; Genkan installs it and Yukimi keeps it up to date. Underneath is what NixOS does best: every change builds a new generation, and the ones before it stay in the boot menu.</p>
    <p class="cta"><a class="button" href="/download/">Download Sekka</a><a href="/aizome/">Tour Aizome</a></p>
  </div>
  {shot}
</section>
"#,
        lockup = site
            .brand
            .inline("sekka-lockup-night.svg", "hero-lockup", "Sekka 雪花")?,
        shot = site.shot(
            "aizome-desktop.png",
            "The Aizome desktop: Sumi editing a Markdown file, Files and Hotaru, Aizome's terminal, running fastfetch, stacked in one tab group beside it, the menu bar along the top, and a notification filed under Tenki in the Deskbar.",
            "Aizome: Sumi, with Files and Hotaru, its terminal, stacked in one tab group, in a test VM built from Sekka's own modules.",
            "win hero-shot",
        )?,
    )?;

    write!(
        b,
        r#"<section class="win focus flagship" aria-labelledby="aizome">
  <div class="flag-head">
    <p class="eyebrow">The desktop · Aizome {aizome}</p>
    <h2 id="aizome">Aizome, the desktop Sekka is made for.</h2>
    <p class="lead">Aizome is indigo dyeing: cloth dipped again and again, each dip adding depth. It is Sekka's own desktop, written in Rust on its own toolkit, Sukumo {sukumo}, from the compositor to the calculator. Its ideas come from Haiku, NeXTSTEP and IRIX.</p>
  </div>
  <ul class="facts flag-points">
    <li><strong>Tabs, from Haiku.</strong> Every window wears a tab only as wide as its title. Drop one tab among another window's and the windows stack into a group, from any apps.</li>
    <li><strong>Menus, from NeXT.</strong> One menu bar for the app you're using. <kbd>Super</kbd> + right-click opens its whole menu at the pointer, and any menu tears off into a palette.</li>
    <li><strong>A palette for everything.</strong> <kbd>Super</kbd> + <kbd>Space</kbd> finds every command in the app's menus and teaches you its shortcut, with windows, files, apps, settings and arithmetic.</li>
    <li><strong>The Toolchest and Desks, from IRIX.</strong> The Deskbar, from Haiku, lists each app with its windows and files its notifications beneath it.</li>
    <li><strong>Super is the command key</strong> in every app, and each keyboard keeps its own modifiers, so it sits under your thumb on a Mac keyboard and a PC keyboard alike.</li>
    <li><strong>Fast by construction.</strong> Integer scaling, opaque surfaces, damage-only redraws, and a 4 ms budget for every frame of the desktop and its apps.</li>
  </ul>
  <div class="pair">{menu}{palette}</div>
  <p class="flag-links"><a href="/aizome/">More about Aizome</a><a href="{AIZOME}/">Every app in detail, at aizome.computer</a></p>
</section>
"#,
        aizome = site.ja("藍染")?,
        sukumo = site.ja("蒅")?,
        menu = site.shot(
            "aizome-menu.png",
            "Aizome with Sumi's whole menu open at the pointer, every menu of the app as a submenu.",
            "Super + right-click: the focused app's whole menu at the pointer.",
            "",
        )?,
        palette = site.shot(
            "aizome-palette.png",
            "Aizome's command palette over the desktop, with find typed and the matching commands, windows and files listed.",
            "Super + Space: the command palette, with find typed.",
            "",
        )?,
    )?;

    write!(
        b,
        r#"<section class="floor" aria-labelledby="rooms">
  <h2 id="rooms" class="floor-title">Come in. Sit down. Watch the snow.</h2>
  <article class="win mat mat-wide-top">
    <header class="mat-head"><h3>Genkan {genkan}</h3><p>the entrance</p></header>
    <div class="mat-body split">
      <p>Choose your disk, account, desktops, applications, location and keyboard, then review everything before anything is erased. Press the key beside your space bar and Genkan offers to put Super under your thumb. Genkan builds and caches the system while you review, so the installation itself is short. It is an Aizome app, opened by the live session. It can bring your public SSH keys and Git identity from GitHub, carry the live session's Wi-Fi into the new system, and encrypt the disk.</p>
      {genkan_shot}
    </div>
  </article>
  <article class="win mat mat-tall-right">
    <header class="mat-head"><h3>Tatami {tatami}</h3><p>a room for the keyboard</p></header>
    <div class="mat-body">
      {tatami_shot}
      <p>Sekka's other desktop, for when you'd rather tile. Hyprland driven from the keyboard, inspired by Omarchy and adapted for NixOS: a compact bar, a searchable menu, notifications, Displays, Network, Activity and Keep Awake, in Tokyo Night with square corners.</p>
      <p><a href="/tatami/">More about Tatami</a></p>
    </div>
  </article>
  <div class="win focus mat mat-half">
    {crest}
    <p><span class="mat-kanji">{sekka}</span> Sekka is an Edo-period word for snow crystals and the patterns drawn from them. The crest is a <em>yukiwa</em>, the snow ring of Japanese family crests.</p>
    <p><a href="/design/">The design</a></p>
  </div>
  <article class="win mat mat-tall-left">
    <header class="mat-head"><h3>Yukimi {yukimi}</h3><p>a window for watching the snow</p></header>
    <div class="mat-body">
      {yukimi_shot}
      <p>See what's installed and why, find software, update, go back to an earlier generation and free up space, without editing Nix by hand. Yukimi explains what Nix is doing as it goes, looks the same on every Sekka desktop, and works on any NixOS system, flake or channels.</p>
      <p><a href="/yukimi/">More about Yukimi</a></p>
    </div>
  </article>
  <article class="win mat mat-wide-bottom">
    <header class="mat-head"><h3>Or a desktop you already know</h3></header>
    <div class="mat-body">
      <p>Plasma, GNOME, Xfce and vanilla Hyprland are there too, beside Aizome and Tatami. Install several and pick one at the login screen; they share your account.</p>
      <div class="pair">{plasma}{gnome}</div>
    </div>
  </article>
</section>
"#,
        genkan = site.ja("玄関")?,
        tatami = site.ja("畳")?,
        yukimi = site.ja("雪見")?,
        sekka = site.ja("雪花")?,
        crest = site
            .brand
            .inline("sekka-crest-blue.svg", "half-crest", "The Sekka crest")?,
        genkan_shot = site.shot(
            "genkan.png",
            "Genkan's welcome page: nine steps down the side, from Welcome to Install, and a warning that the installer erases one whole disk.",
            "Genkan's preview with sample data, in Aizome.",
            "",
        )?,
        tatami_shot = site.shot(
            "tatami-menu.png",
            "Tatami's menu over an empty desktop: Apps, Learn, Trigger, Style, Setup, About and System.",
            "Tatami's menu, opened with Super + Space.",
            "",
        )?,
        yukimi_shot = site.shot(
            "aizome-yukimi.png",
            "Yukimi's overview in Aizome: the machine's name, its packages, the store's size and its generations.",
            "Yukimi in Aizome, reading a test VM named sekka-demo.",
            "",
        )?,
        plasma = site.shot(
            "plasma.png",
            "Sekka's Plasma session with Konsole running fastfetch over Dolphin.",
            "Plasma, in a test VM.",
            "",
        )?,
        gnome = site.shot(
            "gnome.png",
            "Sekka's GNOME session with Files over Yukimi's earlier GTK interface.",
            "GNOME in a test VM, with Yukimi's earlier interface.",
            "",
        )?,
    )?;

    b.push_str(
        r#"<section class="ledger" aria-label="What Sekka is good at, and what to know before installing">
  <div class="win">
    <h2>What it's good at</h2>
    <ul class="facts good">
      <li><strong>Every change is a generation.</strong> Updates, configuration changes and rollbacks each build one, and earlier ones stay in Yukimi's History and the boot menu.</li>
      <li><strong>The install is prepared before your disk is touched.</strong> Genkan builds the system while you review, then copies it.</li>
      <li><strong>An update is one action.</strong> Sekka decides whether a change can apply now or should wait for a restart, and it never restarts or signs you out on its own.</li>
      <li><strong>Updates download, they don't compile.</strong> Sekka's own packages come from a public, signed binary cache; only NVIDIA's driver is built for your kernel.</li>
      <li><strong>Docking behaves.</strong> Close the lid with a screen attached and the laptop keeps working on the other screen; windows stranded by an unplug come back.</li>
      <li><strong>Keep Awake tells the truth.</strong> It checks that sleep is really held off, and says so when it can't.</li>
    </ul>
  </div>
  <div class="win">
    <h2>Before you install</h2>
    <ul class="facts">
      <li><strong>Sekka takes a whole disk.</strong> Genkan erases the disk you choose; there's no dual boot or manual partitioning, so keep your files elsewhere.</li>
      <li><strong>Installing needs an internet connection.</strong></li>
      <li><strong>For x86_64 PCs</strong>, with UEFI or legacy BIOS, on ext4, Btrfs or XFS, with optional LUKS2 encryption. Turn Secure Boot off.</li>
      <li><strong>Nightly builds.</strong> Each <a href="/download/">ISO</a> is published once that exact image has been installed, booted, logged into and used.</li>
    </ul>
    <p><a href="/status/">How each part is tested</a></p>
  </div>
</section>
"#,
    );
    Ok(Page {
        path: "/",
        title: "Sekka: NixOS you can look after without a text editor".into(),
        description: "Sekka is NixOS with a desktop of its own, Aizome, a graphical installer, and Yukimi for software and updates.",
        body: b,
    })
}

pub fn aizome(site: &Site) -> Result<Page> {
    let mut b = String::new();
    write!(
        b,
        r#"<section class="win focus page-head">
  <img class="page-icon" src="{icon}" width="96" height="96" alt="">
  <div>
    <p class="eyebrow">Aizome {kanji}</p>
    <h1>A desktop of its own, from the compositor to the calculator.</h1>
    <p class="lead"><em>Aizome</em> is indigo dyeing: cloth dipped again and again, each dip adding depth. It is also Sekka's desktop, the one Sekka is made for: a Wayland desktop written in Rust on its own toolkit, Sukumo {sukumo}, taking its ideas from Haiku, NeXTSTEP and IRIX.</p>
    <p class="cta"><a class="button" href="{AIZOME}/">Visit aizome.computer →</a><a href="/download/">Download Sekka</a></p>
  </div>
</section>
{shot}
"#,
        kanji = site.ja("藍染")?,
        sukumo = site.ja("蒅")?,
        icon = site.asset("apps/Toolchest.svg")?,
        shot = site.shot(
            "aizome-desktop.png",
            "The Aizome desktop: Sumi editing a Markdown file, Files and Hotaru, Aizome's terminal, running fastfetch, stacked in one tab group beside it, the menu bar along the top, and a notification filed under Tenki in the Deskbar.",
            "Sumi, with Files and Hotaru, Aizome's terminal, stacked in one tab group beside it, in a test VM built from Sekka's own modules.",
            "win wide-shot",
        )?,
    )?;
    b.push_str(
        r#"<section class="tiles">
  <article class="win">
    <h2>Tabs and groups</h2>
    <p>Every window wears a tab only as wide as its title, as Haiku's do. A new window of the app you're using opens as another tab on its window. Drop one tab among another window's and the windows stack into a group, from any apps, the tab going where you dropped it. Drag a group's tab to reorder it (<kbd>Alt</kbd>-drag for the one showing), and slide a tab along the top edge with <kbd>Shift</kbd>.</p>
  </article>
  <article class="win">
    <h2>One menu bar</h2>
    <p>The menu bar shows the menus of the app you're using, as NeXT's did. Any menu tears off into a palette that stays while its app is focused, or always, once pinned, and palettes are remembered between sessions. Services act on selected text: make a note of it, open it in Sumi, search the web for it, or calculate it.</p>
  </article>
  <article class="win">
    <h2>Toolchest, Deskbar and Desks</h2>
    <p>The Toolchest, from IRIX, opens applications by category, places, Desks and the session. The Deskbar, from Haiku, lists each app with its windows and files its notifications beneath it; drag its strip to stand it at the left or right, and its edge to make it wider, slimmer or icons only. Desks keep work apart, and <kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>↑</kbd> shows them all.</p>
  </article>
  <article class="win">
    <h2>Made for the keyboard</h2>
    <p>Aizome takes as much from modern tiling window managers as from Haiku. Fill the screen with a window or split it left or right from the keyboard, move between numbered Desks and carry windows with you, switch apps and their windows, and open any window's menu with a key for every move. Every command in every app is in the palette, which teaches you its shortcut as you go.</p>
  </article>
  <article class="win">
    <h2>Every keyboard, set up for you</h2>
    <p><kbd>Super</kbd> is the command key in every app: <kbd>Super</kbd> + <kbd>C</kbd> copies, in the terminal too. Each keyboard keeps its own modifier keys, Caps Lock and Compose key. Plug one in or pair it, press the key beside its space bar, and Settei suggests where Super should go, so it sits under your thumb on a Mac keyboard and a PC keyboard alike.</p>
  </article>
  <article class="win">
    <h2>One file chooser</h2>
    <p>Browsers, Electron apps, Zed and Flatpaks open and save files in Aizome's own chooser: Files' sidebar and path bar over the app's window, with its filters and options.</p>
  </article>
  <article class="win">
    <h2>Fast by construction</h2>
    <p>Integer scaling, opaque surfaces, damage-only redraws, and a budget of 4 ms for every frame of the desktop and its apps. Aizome composites on the graphics card, and in software when it has to: the boot menu's recovery entry draws the same session without a GPU driver.</p>
  </article>
</section>
"#,
    );
    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="menus">
  <div class="win step-text">
    <h2 id="menus">The whole menu, at the pointer</h2>
    <p><kbd>Super</kbd> + right-click anywhere, or right-click the empty desktop, and the focused app's whole menu opens at the pointer: the app's name in its title strip, every menu as a submenu, Hide and Quit at the bottom. Drag the title strip and it tears off into a palette.</p>
    <p>Menus answer the keyboard too: <kbd>F10</kbd> focuses the menu bar, the arrows move, <kbd>Return</kbd> chooses.</p>
  </div>
  {menu}
</section>
<section class="tour-step" aria-labelledby="palette">
  <div class="win step-text">
    <h2 id="palette">A palette for everything</h2>
    <p><kbd>Super</kbd> + <kbd>Space</kbd> opens the command palette on the screen you're using. It searches every command in the focused app's menus and shows each one's shortcut, so you learn it as you go, along with windows, files, apps, settings and arithmetic. <kbd>Tab</kbd> narrows it to one of them.</p>
    <p>Start typing in the Toolchest and it becomes the palette.</p>
  </div>
  {palette}
</section>
"#,
        menu = site.shot(
            "aizome-menu.png",
            "Aizome with Sumi's whole menu open at the pointer, every menu of the app as a submenu.",
            "Sumi's whole menu, opened with Super + right-click, in the same test VM.",
            "win",
        )?,
        palette = site.shot(
            "aizome-palette.png",
            "Aizome's command palette over the desktop, with find typed and the matching commands, windows and files listed.",
            "The command palette (Super + Space) with find typed, in the same test VM.",
            "win",
        )?,
    )?;
    b.push_str(
        r#"<section class="win" aria-labelledby="aizome-keys">
  <h2 id="aizome-keys">The keys you'll want first</h2>
  <table class="keys">
    <tbody>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Space</kbd></th><td>The command palette</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + right-click</th><td>The focused app's whole menu, at the pointer</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Alt</kbd> + <kbd>Space</kbd></th><td>The Toolchest</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Return</kbd></th><td>The default terminal, Hotaru unless Settei names another, in the Desk's folder</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>C</kbd> / <kbd>V</kbd> / <kbd>Z</kbd></th><td>Copy, paste and undo, in every app</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Tab</kbd></th><td>Switch apps, then that app's windows</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Alt</kbd> + <kbd>↑</kbd> / <kbd>←</kbd> / <kbd>→</kbd></th><td>Fill the screen, or split it left or right</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>Space</kbd></th><td>The window's own menu, a key for every move</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Shift</kbd> + <kbd>[</kbd> / <kbd>]</kbd></th><td>The previous or next tab in a group</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>S</kbd></th><td>Stack this window with the one behind it</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>1</kbd>…<kbd>9</kbd></th><td>Go to a Desk; with <kbd>Shift</kbd>, take the window along</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>↑</kbd></th><td>Every Desk</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>F</kbd></th><td>Full screen</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>H</kbd> / <kbd>M</kbd> / <kbd>Q</kbd></th><td>Hide, minimise or quit the app</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>Q</kbd></th><td>Lock the screen</td></tr>
    </tbody>
  </table>
</section>
"#,
    );
    let mut apps = String::new();
    for (name, kanji, generic, what) in APPS {
        write!(
            apps,
            r#"<li class="app"><img src="{icon}" width="48" height="48" alt=""><div><h3>{name} <span class="app-ja">{ja}</span></h3><p class="app-generic">{generic}</p><p>{what}</p></div></li>"#,
            icon = site.asset(&format!("apps/{name}.svg"))?,
            ja = site.ja(kanji)?,
            what = esc(what),
        )?;
    }
    write!(
        b,
        r#"<section class="win" aria-labelledby="apps">
  <h2 id="apps">Its own apps</h2>
  <p>Aizome comes with its own apps, each named with a Japanese word for what it's for, and each built on Sukumo: the same menus, the same palette, Night and Day, and every command reachable from the keyboard.</p>
  <ul class="apps">{apps}</ul>
  <p><a href="{AIZOME}/">Every app in detail, at aizome.computer</a></p>
</section>
"#
    )?;
    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="settei">
  <div class="win step-text">
    <h2 id="settei">Settei, the settings</h2>
    <p>Settei is laid out as a Mac's System Settings: a search field, your account, then panes in groups, each pane a form beside them. Every option can be searched, here or from the palette.</p>
    <p>Network, Bluetooth, sound, displays, appearance in Night, Day or High Contrast with a focus colour of your choosing, Desks and the Deskbar, the menu bar, power, the lock screen, accounts and keyboards are all here, and Settei sets up each new keyboard you plug in.</p>
  </div>
  {settei}
</section>
<section class="tiles">
  <article class="win">
    <h2>Hotaru {hotaru}, the terminal</h2>
    <p><kbd>Super</kbd> + <kbd>Return</kbd> opens Hotaru, Aizome's own terminal, and programs that need a terminal open in it too. It is built with Aizome's toolkit, not GTK or a web view: native menus, tabs of independently focused split panes, and search through the scrollback. It opens in fish, set up as Omarchy sets it up: a Starship prompt, vi keys, and fzf searches of your history and files.</p>
    <p>foot is installed too: choose it in Settei, under General › Default Apps › Terminal.</p>
  </article>
  <article class="win">
    <h2>The login screen</h2>
    <p>Aizome has its own login screen, and it lists every desktop you installed: Tatami, Plasma and the others share your account. With GNOME installed, GDM takes its place, since GNOME locks its screen only under GDM.</p>
    <p>Yukimi's Desktops page adds and removes desktops later, and chooses which one starts.</p>
  </article>
</section>
"#,
        hotaru = site.ja("蛍")?,
        settei = site.shot(
            "aizome-settings.png",
            "Settei, Aizome's Settings, at its Appearance pane, with the search field, the account card and the panes in groups down its sidebar.",
            "Settei at Appearance, cropped to its window with its tab, frame and shadow, in the same test VM.",
            "win",
        )?,
    )?;
    b.push_str(
        r#"<section class="win" aria-labelledby="anywhere">
  <h2 id="anywhere">On any NixOS system</h2>
  <p>On Sekka, choose Aizome in the installer, where it is selected by default, or add it later in Yukimi's Desktops page. On any NixOS system with flakes, add Sekka's flake, import the module and enable it; then pick Aizome on the login screen.</p>
  <pre class="code"><code>{
  inputs.sekka.url = "github:bitemyapp/sekka/nightly";
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.example = nixpkgs.lib.nixosSystem {
      modules = [
        sekka.nixosModules.aizome
        { programs.aizome.enable = true; }
      ];
    };
  };
}</code></pre>
</section>
"#,
    );
    write!(
        b,
        r#"<section class="win focus cta-band" aria-labelledby="more">
  <div>
    <h2 id="more">Every app, in detail.</h2>
    <p class="lead">Aizome has a site of its own, <a href="{AIZOME}/">aizome.computer</a>, which takes each of its apps, the desktop and its keys further than this page can.</p>
  </div>
  <p class="cta"><a class="button" href="{AIZOME}/">Visit aizome.computer →</a><a href="/download/">Download Sekka</a></p>
</section>
"#
    )?;
    Ok(Page {
        path: "/aizome/",
        title: "Aizome: Sekka's own desktop".into(),
        description: "Aizome is Sekka's desktop, written in Rust from the compositor up: Haiku's tabs, NeXT's menus, IRIX's Toolchest, a command palette for everything, and its own apps.",
        body: b,
    })
}

pub fn tatami(site: &Site) -> Result<Page> {
    let mut b = String::new();
    write!(
        b,
        r#"<section class="win focus page-head">
  <img class="page-icon" src="{icon}" width="96" height="96" alt="">
  <div>
    <p class="eyebrow">Tatami {kanji}</p>
    <h1>A keyboard-driven desktop, square and quiet.</h1>
    <p class="lead">Tatami is Sekka's other desktop, for people who would rather tile windows and drive everything from the keyboard. It is Hyprland, inspired by Omarchy 4 and adapted for NixOS without home-manager: Waybar, Walker, Mako, SwayOSD and hyprlock styled to match, and one Rust program, <code>tatami</code>, in place of Omarchy's scripts. It is not Omarchy and is not affiliated with Omarchy or Basecamp.</p>
  </div>
</section>
{shot}
"#,
        kanji = site.ja("畳")?,
        icon = site.asset("brand/tatami.svg")?,
        shot = site.shot(
            "tatami-menu.png",
            "The Tatami desktop with its menu open: Apps, Learn, Trigger, Style, Setup, About and System, under the bar and over the wallpaper.",
            "Tatami's menu (Super + Space) over an empty desktop, in a test VM built from Sekka's own modules.",
            "win wide-shot",
        )?,
    )?;
    b.push_str(
        r#"<section class="win" aria-labelledby="which">
  <h2 id="which">Tatami or Aizome?</h2>
  <div class="cols which">
    <div>
      <h3><a href="/aizome/">Aizome</a></h3>
      <p>The desktop Sekka is made for: windows you arrange yourself, stacked into tab groups, with one menu bar, a command palette and its own apps. It takes as much from modern tiling window managers as from Haiku, and goes out of its way to stay under your fingers. Anything not on a key is in the palette, which teaches you its shortcut, and every app can be driven from the keyboard.</p>
      <ul class="facts">
        <li><kbd>Super</kbd> + <kbd>Alt</kbd> and an arrow fills the screen with a window, or splits it left or right.</li>
        <li><kbd>Super</kbd> + <kbd>Ctrl</kbd> and a number goes to a Desk; add <kbd>Shift</kbd> to take the window along.</li>
        <li><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>Space</kbd> opens the window's own menu, with a key for every move.</li>
      </ul>
      <p><a href="/aizome/#aizome-keys">All of Aizome's keys</a></p>
    </div>
    <div>
      <h3>Tatami</h3>
      <p>Tatami tiles every window, always: each takes its share of the screen, and everything is a key away. It is Hyprland as Omarchy taught it, adapted for NixOS.</p>
      <ul class="facts">
        <li>Workspaces 1 to 5, numbered in the bar, a key each.</li>
        <li>One menu, on <kbd>Super</kbd> + <kbd>Space</kbd>, for apps, settings and the system.</li>
        <li>Tokyo Night, square corners, no shadows.</li>
      </ul>
      <p>The installer selects both; leave either out there, or change it later in Yukimi's Desktops page, and pick one at the login screen.</p>
    </div>
  </div>
</section>

<section class="tiles">
  <article class="win">
    <h2>The look</h2>
    <p>Tokyo Night throughout, JetBrains Mono, square corners and a 2px blue border on the focused window. Windows sit 5px apart and 10px from the screen edge. No blur, no shadows. Twenty-three wallpapers come with recorded sources; Omarchy's own were left out because their licenses are unclear.</p>
  </article>
  <article class="win">
    <h2>The bar</h2>
    <p>Twenty-six pixels: the menu, workspaces 1 to 5, a centred clock with recording, night-light, do-not-disturb and Keep Awake indicators, the weather, then the tray, Bluetooth, network, audio, display and battery. The header of this site is a copy of it.</p>
  </article>
  <article class="win">
    <h2>Menu and launcher</h2>
    <p>One menu holds Apps, Learn, Trigger, Style, Setup, About and System. Typing searches menu entries and applications together. Applications run as services of their own: a memory-pressure kill of one cannot end the session, and restarting the launcher cannot end them.</p>
  </article>
  <article class="win">
    <h2>Notifications</h2>
    <div class="note-demo" role="img" aria-label="A Tatami notification: Tatami reloaded. Hyprland's configuration applies from the next login.">
      <strong>Tatami reloaded</strong>
      <span>Hyprland's configuration applies from the next login</span>
    </div>
    <p>380px cards with a 2px accent border, and an on-screen display at the bottom for volume and brightness.</p>
  </article>
</section>

<section class="win" aria-labelledby="keys">
  <h2 id="keys">The keys you'll want first</h2>
  <table class="keys">
    <tbody>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Space</kbd></th><td>The Tatami menu</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Alt</kbd> + <kbd>Space</kbd></th><td>Applications</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Return</kbd></th><td>A terminal</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>K</kbd></th><td>Every key binding</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Escape</kbd></th><td>Lock, suspend, restart, shut down</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>C</kbd> / <kbd>V</kbd> / <kbd>X</kbd></th><td>Copy, paste and cut, in terminals too</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>Space</kbd></th><td>Change the background</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>D</kbd></th><td>Displays</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>I</kbd></th><td>Keep Awake</td></tr>
      <tr><th scope="row"><kbd>Super</kbd> + <kbd>R</kbd></th><td>Reload Tatami after an update, without logging out</td></tr>
    </tbody>
  </table>
</section>

<section class="tiles">
  <article class="win">
    <h2>Laptops and docks</h2>
    <p>Close the lid with an external display connected and the built-in display turns off, its windows move to the other screen, and the laptop keeps running, on battery too. Open the lid and it comes back where you arranged it. A display you turn off stays off whenever the same displays are connected again, until you turn it on or log in again. If a display change leaves a window off screen, Tatami brings it back.</p>
    <p>Lid handling and docking are in daily use with external displays, and tests with simulated displays and a simulated lid guard them between changes.</p>
  </article>
  <article class="win">
    <h2>Keep Awake</h2>
    <p>For long downloads, builds or overnight work, turn on Keep Awake with the coffee cup in the bar or <kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>I</kbd>. It holds off sleep, automatic locking and lid-close sleep until you turn it off, and remembers the choice at your next login.</p>
    <p>It checks that the hold is real: if it ever can't, the cup turns red so you know.</p>
  </article>
</section>

"#,
    );
    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="displays">
  <div class="win step-text">
    <h2 id="displays">Displays</h2>
    <p>The bar's display icon or <kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>D</kbd> opens Displays, after macOS's Displays settings, on the screen you are using. The arrangement is drawn to scale: drag a display and it goes beside the others, sharing an edge and lining up with theirs. The main display, where workspace 1 opens, has the menu bar.</p>
    <p>For each display: main, extended, a mirror of another, or off; resolution as sizes from larger text to more space, or every resolution and scale; refresh rate and variable refresh rate; rotation; HDR, colour profile and 10-bit colour; and the built-in display's brightness. Night Shift and its colour temperature are here too. Changes apply at once, and those that can leave a screen blank are undone after 15 seconds unless you keep them.</p>
  </div>
  {shot}
</section>
"#,
        shot = site.shot(
            "displays.png",
            "Tatami's Displays window: the VM's one display in the arrangement, its name, size and refresh rate, Use as main display, and resolution as five sizes from Larger Text to More Space.",
            "The Displays window in a test VM, with its one virtual display.",
            "win",
        )?,
    )?;
    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="network">
  <div class="win step-text">
    <h2 id="network">Network</h2>
    <p>The network icon opens its panel at once, with the networks already seen; a fresh scan arrives for the next look. Network settings… opens the Network window, built as Displays is and laid out after macOS's Network settings.</p>
    <p>Join a network with its password, an organisation's username and password, or a hidden one by its name. Wired connections and VPNs sit beside Wi-Fi, each VPN with its switch, and a WireGuard VPN is added from the file your provider gives you. Every connection has a page of its own for joining, metering, a private hardware address, IPv4, DNS and IPv6, and changes apply at once.</p>
  </div>
  {shot}
</section>
"#,
        shot = site.shot(
            "network.png",
            "Tatami's Network window: Wi-Fi on and joined to Home, a known network and two others in range, a WireGuard VPN switched off, and the saved networks.",
            "The Network window in a test VM, on simulated Wi-Fi: Home, Café Corner, Riverside Library and Studio 5G are made-up names.",
            "win",
        )?,
    )?;
    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="activity">
  <div class="win step-text">
    <h2 id="activity">Activity</h2>
    <p><kbd>Super</kbd> + <kbd>Ctrl</kbd> + <kbd>T</kbd> opens Activity, after macOS's Activity Monitor, with the application you were using selected. Each application shows with all its processes together, a browser's dozens of helpers or a terminal and what runs in it, sorted by processor, memory, estimated energy, graphics or disk use.</p>
    <p>Beside the list, the inspector follows the selected application as it changes, with its processor and memory over the last five minutes; under the list, the system as a whole. Quit, Force Quit, Pause and Resume are in the inspector and the right-click menu, and quitting asks first. Linux measures power per device, so Activity shares energy out from the processor's and graphics cards' meters.</p>
  </div>
  {shot}
</section>
"#,
        shot = site.shot(
            "activity-memory.png",
            "Tatami's Activity window on its Memory view: each application's proportional, private, resident and shared memory, Hyprland first and the selected terminal second, and memory use and pressure under the list.",
            "Activity's Memory view (Ctrl + 2) in a test VM. Dashes are root's processes, whose memory maps a user can't read; Hyprland's memory includes Mesa drawing in software.",
            "win",
        )?,
    )?;
    b.push_str(
        r#"
<section class="win" aria-labelledby="anywhere">
  <h2 id="anywhere">On any NixOS system</h2>
  <p>Tatami works without the rest of Sekka. Add Sekka's flake, import the module and enable it; a Tatami session appears on the login screen beside your other desktops, which it leaves alone.</p>
  <pre class="code"><code>{
  inputs.nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1";
  inputs.sekka = {
    url = "github:bitemyapp/sekka/nightly";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.example = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        sekka.nixosModules.tatami
        { programs.tatami.enable = true; }
        ./configuration.nix
      ];
    };
  };
}</code></pre>
  <p>You need a display manager that offers Wayland sessions. Sekka's binary cache is configured by the full Sekka system, not by this module, so add the cache yourself to download Tatami's packages rather than build them.</p>
</section>

<section class="win" aria-labelledby="leaves-out">
  <h2 id="leaves-out">What it leaves out of Omarchy</h2>
  <ul class="facts">
    <li>Arch package management, Limine and snapper, Plymouth theming and the first-run script: NixOS and Sekka do those their own way.</li>
    <li>Theme and font switching, web apps, the webcam overlay, sharing, reminders, dictation and AI agents.</li>
    <li>A notification history panel and network statistics.</li>
  </ul>
</section>
"#,
    );
    Ok(Page {
        path: "/tatami/",
        title: "Tatami: a keyboard-driven desktop for NixOS".into(),
        description: "Tatami is Sekka's tiling desktop: Hyprland inspired by Omarchy, with Tokyo Night, square corners, a 26px bar, one menu, Displays, Network, Activity and Keep Awake.",
        body: b,
    })
}

/// How a colour is judged, and against what.
enum Role {
    Ground,
    Text,
    OnAccent,
    Mark,
    Cloth,
    Edge,
}

fn role(name: &str) -> Role {
    match name {
        "surface" | "surface-raised" | "selection" => Role::Ground,
        "on-accent" => Role::OnAccent,
        "crest" => Role::Mark,
        "indigo" => Role::Cloth,
        "border-inactive" => Role::Edge,
        _ => Role::Text,
    }
}

fn grade(ratio: f64, contrast_theme: bool, role: &Role) -> (&'static str, &'static str) {
    match role {
        Role::Edge | Role::Mark if ratio >= 3.0 => ("pass", "meets 3:1"),
        Role::Edge | Role::Mark => ("note", "decorative"),
        _ if ratio >= 7.0 => ("pass", "AAA"),
        _ if contrast_theme && ratio >= 4.5 => ("note", "AA, short of AAA"),
        _ if ratio >= 4.5 => ("pass", "AA"),
        _ if ratio >= 3.0 => ("note", "large text only"),
        _ => ("note", "decorative only"),
    }
}

fn palette(site: &Site) -> Result<String> {
    let t = &site.tokens;
    let mut s = String::from(
        r#"<div class="table-wrap"><table class="palette"><caption>Every colour in every theme. Ratios are computed when the site is built, against the darkest or lightest of the surfaces each colour sits on.</caption><thead><tr><th scope="col">Token</th>"#,
    );
    for th in &t.themes {
        write!(s, r#"<th scope="col">{}</th>"#, esc(&th.name))?;
    }
    s.push_str("</tr></thead><tbody>");
    for c in &t.colors {
        let r = role(&c.name);
        write!(
            s,
            r#"<tr><th scope="row"><code>{}</code><span class="usage">{}</span></th>"#,
            esc(&c.name),
            esc(&c.usage)
        )?;
        for th in &t.themes {
            let hex = t.hex(&c.name, &th.id)?;
            let hc = th.id == "contrast";
            let verdict = match r {
                Role::Ground => String::from(r#"<span class="grade">ground</span>"#),
                Role::OnAccent => {
                    let ratio = t.contrast(&c.name, "accent", &th.id)?;
                    let (k, w) = grade(ratio, hc, &r);
                    format!(r#"<span class="grade {k}">{ratio:.1}:1 on accent · {w}</span>"#)
                }
                Role::Cloth => {
                    let ratio = t.contrast("text-strong", &c.name, &th.id)?;
                    let (k, w) = grade(ratio, hc, &Role::Text);
                    format!(r#"<span class="grade {k}">text-strong {ratio:.1}:1 · {w}</span>"#)
                }
                _ => {
                    let ratio = GROUNDS
                        .iter()
                        .map(|g| t.contrast(&c.name, g, &th.id))
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .fold(f64::INFINITY, f64::min);
                    let (k, w) = grade(ratio, hc, &r);
                    format!(r#"<span class="grade {k}">{ratio:.1}:1 · {w}</span>"#)
                }
            };
            write!(
                s,
                r#"<td><span class="sw sw-{name}-{theme}" aria-hidden="true"></span><code>{hex}</code>{verdict}</td>"#,
                name = c.name,
                theme = th.id,
                hex = esc(&hex),
            )?;
        }
        s.push_str("</tr>");
    }
    s.push_str("</tbody></table></div>");
    Ok(s)
}

fn plain_tokens(site: &Site) -> String {
    let mut s = String::from(r#"<dl class="plain">"#);
    for (family, list) in &site.tokens.plain {
        for p in list {
            let _ = write!(
                s,
                r#"<div><dt><code>{}</code> <span class="fam">{}</span></dt><dd><code>{}</code> {}</dd></div>"#,
                esc(&p.name),
                esc(family),
                esc(&p.value),
                esc(&p.usage)
            );
        }
    }
    s.push_str("</dl>");
    s
}

pub fn design(site: &Site) -> Result<Page> {
    let mut b = String::new();
    write!(
        b,
        r#"<section class="win focus page-head">
  <div>
    <p class="eyebrow">The design {kanji}</p>
    <h1>Tatami's Tokyo Night, made into a system.</h1>
    <p class="lead">A tour of Sekka's brand: the crest and where it comes from, Tatami's colours with their contrast measured, the type, the shapes and spaces the desktop already uses, and what isn't finished. This page is built from the same token file as the brand.</p>
  </div>
</section>

<form class="win themes" aria-labelledby="themes-legend">
  <fieldset>
    <legend id="themes-legend">See this page in</legend>
    <input type="radio" name="theme" id="theme-auto" checked><label for="theme-auto">the bar's choice</label>
    <input type="radio" name="theme" id="theme-night"><label for="theme-night">Night</label>
    <input type="radio" name="theme" id="theme-day"><label for="theme-day">Day</label>
    <input type="radio" name="theme" id="theme-contrast"><label for="theme-contrast">High contrast</label>
  </fieldset>
  <p>The site is Night until you switch it to Day in the bar, which your browser remembers; High contrast comes when your system asks for more contrast. A choice here is only for this page, while you're on it.</p>
</form>

<section class="tour-step" aria-labelledby="crest">
  <div class="win step-text">
    <p class="step-n">1</p>
    <h2 id="crest">The crest</h2>
    <p>{sekka_old}, <em>sekka</em>, is an Edo-period word for snow crystals. In 1832 Doi Toshitsura published eighty-six of them, drawn through a microscope, as <em>Sekka Zusetsu</em>, and the patterns became a fashion in Edo design.</p>
    <p>The mark is a <em>yukiwa</em> {yukiwa}, the snow ring of Japanese family crests: six circles of radius 76, centred 36 from the middle of a 256 grid, joined into a softly scalloped ring. A six-armed crystal is cut through it: arms from radius 19 to 84, one pair of branches on each, and a hollow hexagon at the centre. Arms point straight up.</p>
    <p>It is drawn to stay clearly apart from NixOS's lambda snowflake. Sekka runs on NixOS; the two marks are never locked up together.</p>
  </div>
  <figure class="win step-figure">{construction}<figcaption>Six circles of radius 76 (dashed), centred 36 from the middle (dots), make the ring. The crystal is cut through it.</figcaption></figure>
</section>
"#,
        kanji = site.ja("雪花")?,
        yukiwa = site.ja("雪輪")?,
        sekka_old = site.ja("雪華")?,
        construction = crate::brand::construction(),
    )?;

    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="lockups">
  <div class="win step-text">
    <p class="step-n">2</p>
    <h2 id="lockups">Lockups and sizes</h2>
    <p>The wordmark is JetBrains Mono ExtraBold, Tatami's own face, outlined so it never depends on installed fonts. Beside it, {seal} stands vertically like a seal. On this page they take their colours from the theme.</p>
    <p>Below 24px the crest loses its branches: the symbolic glyph thickens the cuts so they survive the pixel grid. The app icon sits on a square tile, because every Tatami surface is square.</p>
    <p class="downloads">SVG: <a href="{night}" download="sekka-lockup-night.svg">lockup, Night</a> · <a href="{day}" download="sekka-lockup-day.svg">lockup, Day</a> · <a href="{crest}" download="sekka-crest-blue.svg">crest</a> · <a href="{symbolic}" download="sekka-symbolic.svg">symbolic</a> · <a href="{app_icon}" download="sekka-app-icon.svg">app icon</a></p>
  </div>
  <div class="win step-figure lockups">
    {lockup}
    <div class="sizes">
      {stacked}
      <div class="glyphs" aria-label="The symbolic glyph at 16, 24 and 32 pixels">{g16}{g24}{g32}</div>
      <img src="{app_icon}" width="96" height="96" alt="The Sekka app icon: the blue crest on a square Night tile.">
    </div>
  </div>
</section>
"#,
        night = site.asset("brand/sekka-lockup-night.svg")?,
        day = site.asset("brand/sekka-lockup-day.svg")?,
        crest = site.asset("brand/sekka-crest-blue.svg")?,
        symbolic = site.asset("brand/sekka-symbolic.svg")?,
        app_icon = site.asset("brand/sekka-app-icon.svg")?,
        lockup = site
            .brand
            .inline("sekka-lockup-night.svg", "demo-lockup", "Sekka 雪花")?,
        stacked = site.brand.inline(
            "sekka-stacked-night.svg",
            "demo-stacked",
            "Sekka 雪花, stacked"
        )?,
        seal = site.ja("雪花")?,
        g16 = site
            .brand
            .inline("sekka-symbolic.svg", "g16", "16 pixels")?,
        g24 = site
            .brand
            .inline("sekka-symbolic.svg", "g24", "24 pixels")?,
        g32 = site
            .brand
            .inline("sekka-symbolic.svg", "g32", "32 pixels")?,
    )?;

    write!(
        b,
        r#"<section class="win step-wide" aria-labelledby="colour">
  <p class="step-n">3</p>
  <h2 id="colour">Colour</h2>
  <div class="cols">
    <p>Night is Tatami's own palette, value for value, from its bar, launcher, notifications, terminal and Displays window. Blue is the one accent: the focused window's border, today in the calendar, links and focus. Status colours always come with a word or an icon.</p>
    <p>Day exists for documents and light pages; Tatami itself is dark only. High contrast is a third theme for people who need more: every text colour reaches 7:1 on every surface, and edges reach 3:1. The one colour that fails in Night is kept on purpose: Tatami's week numbers are 2.8:1, so that grey is only ever decoration.</p>
  </div>
  {palette}
</section>
"#,
        palette = palette(site)?,
    )?;

    let mut type_rows = String::new();
    for g in &site.tokens.groups {
        for st in &g.styles {
            let sample = st.sample.clone().unwrap_or_else(|| st.name.clone());
            let shown = if st.family == "serif-jp" {
                site.ja(&sample)?
            } else {
                esc(&sample)
            };
            write!(
                type_rows,
                r#"<div class="spec"><p class="spec-meta"><code>{name}</code> {size}{lh} · {group}</p><p class="type-{name} spec-sample">{shown}</p></div>"#,
                name = esc(&st.name),
                size = esc(&st.size),
                lh = st
                    .line_height
                    .as_deref()
                    .map(|l| format!("/{}", esc(l)))
                    .unwrap_or_default(),
                group = esc(&g.name),
            )?;
        }
    }
    write!(
        b,
        r#"<section class="win step-wide" aria-labelledby="type">
  <p class="step-n">4</p>
  <h2 id="type">Type</h2>
  <div class="cols">
    <p>JetBrains Mono is the desktop's voice: the bar, the menu, the terminal and the wordmark. Kanji are set in Noto Serif JP, the brush-like mincho beside the mono's straight lines; on this site they are drawn as outlines, so nobody downloads a font for a handful of characters.</p>
    <p>Long reading, like this, is set in Atkinson Hyperlegible Next, which the Braille Institute designed for low-vision readers. Inside the desktop, notifications use Liberation Sans. Every font here is served by this site.</p>
  </div>
  <div class="specimens">{type_rows}</div>
</section>
"#
    )?;

    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="shape">
  <div class="win step-text">
    <p class="step-n">5</p>
    <h2 id="shape">Shape and space</h2>
    <p>Corners are square everywhere; the crest is the one round thing. Borders are 2px: blue for the focused window, notifications and the launcher, a muted grey for windows in the background. Windows are 5px apart and 10px from the edge of the screen. Nothing casts a shadow.</p>
    <p>This site is laid out the same way. Each block is a window; the one in blue has focus. The home page's sections sit like the mats of a four-and-a-half-mat room, where no four corners ever meet.</p>
  </div>
  <div class="win step-figure">
    <div class="room" role="img" aria-label="A four-and-a-half-mat room: four mats around a half mat, the half mat lit like a focused window.">
      <span class="m m1"></span><span class="m m2"></span><span class="m m3"></span><span class="m m4"></span><span class="m m5"></span>
    </div>
    {plain}
  </div>
</section>
"#,
        plain = plain_tokens(site),
    )?;

    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="family">
  <div class="win step-text">
    <p class="step-n">6</p>
    <h2 id="family">The family</h2>
    <p>Genkan's icon is a noren, the split curtain that hangs in a Japanese entrance, dyed indigo with the crest across its slits as real cloth would have it. Tatami's is a four-and-a-half-mat room that also reads as tiled windows. Both are proposals; neither ships yet.</p>
    <p>Yukimi ships Aizome's icon: a square shōji window with falling snow below the paper panes. Its interface and icon are shared across Sekka's desktops.</p>
  </div>
  <div class="win step-figure family">
    <figure><img src="{genkan}" width="128" height="128" alt="Genkan: an indigo noren with the crest across its slits."><figcaption>Genkan, proposed</figcaption></figure>
    <figure><img src="{tatami}" width="128" height="128" alt="Tatami: four mats around a lit half mat."><figcaption>Tatami, proposed</figcaption></figure>
    <figure><img src="{yukimi}" width="128" height="128" alt="Yukimi: a square shōji window onto falling snow."><figcaption>Yukimi, as shipped</figcaption></figure>
  </div>
</section>

<section class="win step-wide" aria-labelledby="unfinished">
  <p class="step-n">7</p>
  <h2 id="unfinished">Still to come</h2>
  <ul class="facts">
    <li><strong>The crest is in Sekka's boot splash</strong>; Tatami's bar still shows the NixOS logo.</li>
    <li><strong>Tatami is dark only.</strong> Day and High contrast exist in the tokens and on this site, not in the desktop.</li>
    <li><strong>The week-number grey fails contrast in Night and Day</strong>, kept because it is Tatami's real value and only decorative.</li>
    <li><strong>Pages share no raster image</strong>, so links posted elsewhere show no picture.</li>
  </ul>
</section>
"#,
        genkan = site.asset("brand/genkan.svg")?,
        tatami = site.asset("brand/tatami.svg")?,
        yukimi = site.asset("brand/yukimi.svg")?,
    )?;
    Ok(Page {
        path: "/design/",
        title: "The Sekka design: crest, colour, type and space".into(),
        description: "A tour of Sekka's brand: the yukiwa crest, Tatami's Tokyo Night palette with measured contrast, the type, the shapes, a High contrast theme, and what's still to come.",
        body: b,
    })
}

pub fn yukimi(site: &Site) -> Result<Page> {
    let mut b = String::new();
    write!(
        b,
        r#"<section class="win focus page-head">
  <img class="page-icon" src="{icon}" width="96" height="96" alt="">
  <div>
    <p class="eyebrow">Yukimi {kanji}</p>
    <h1>Your system, seen from somewhere warm.</h1>
    <p class="lead">A yukimi-shōji is a paper screen with a pane of glass set low in it, so you can sit in a warm room and watch the snow fall in the garden. Yukimi is that window onto a NixOS system: see what is installed and why, find what is available, and change it without opening a text editor.</p>
  <p>Every Sekka desktop uses the same native interface. It opens directly on Wayland; on X11 it opens inside a nested Wayland window. Keyboard commands use Ctrl outside Aizome and Super in Aizome.</p>
  </div>
</section>
{shot}
"#,
        kanji = site.ja("雪見")?,
        icon = site.asset("brand/yukimi.svg")?,
        shot = site.shot(
            "aizome-yukimi.png",
            "Yukimi's overview in Aizome: the machine's name, its package count, how fresh its Nixpkgs is, how much room the store takes, and its generations.",
            "Yukimi in Aizome, reading a test VM named sekka-demo. The dash, the badge and the empty history are that VM's: Yukimi can't date its Nixpkgs or verify its last update there, and says so rather than guessing.",
            "win wide-shot",
        )?,
    )?;
    b.push_str(
        r#"<section class="tiles">
  <article class="win">
    <h2>Overview</h2>
    <p>The system at a glance: its NixOS version and kernel, how many packages it has, how fresh its Nixpkgs is, how much room the store takes, and what changed most recently.</p>
  </article>
  <article class="win">
    <h2>Installed</h2>
    <p>Everything installed, grouped by why: added with Yukimi, listed in your configuration by hand, chosen from a catalog such as the installer's, installed just for you, and everything your desktops and settings bring. Anything in the first four groups can be removed from here, entries you wrote yourself included.</p>
  </article>
  <article class="win">
    <h2>Discover</h2>
    <p>A catalog of well-known applications, and a search of every top-level package in Nixpkgs. Try a package without installing it, install it just for you without a password, or for everyone. Broken and unfree packages are labelled before you pick them.</p>
  </article>
  <article class="win">
    <h2>Updates</h2>
    <p>Where the system comes from: each input of its flake, or each channel, the exact revision it is at, and how old that is. Checking asks every source for its newest version without changing anything, and marks each as up to date, newer, or <em>would go back</em>. Yukimi updates only what moves forward.</p>
    <p>Each source from GitHub, GitLab, SourceHut or Git shows the branch it follows, and can follow another. Working on a project your system is built from comes down to: push, open Updates, press Update.</p>
  </article>
  <article class="win">
    <h2>History</h2>
    <p>Every generation you can go back to. Open one to see what changed from the one before: upgrades, downgrades, additions and removals, with sizes. Going back also brings back the choices it was built from, so your next change starts from there.</p>
  </article>
  <article class="win">
    <h2>Storage</h2>
    <p>The Nix store split into what the running system needs, older generations, user profiles, projects and dev shells, and garbage, with the heaviest packages and every garbage-collector root. Cleaning up is one button.</p>
  </article>
</section>

<section class="tiles">
  <article class="win">
    <h2>Every change is built first</h2>
    <p>A change is built before anything switches to it, so a failed build leaves the running system as it was and the files Yukimi touched are put back. Press Stop and the same happens. Once the new system starts switching, it finishes, so a switch is never cut off halfway.</p>
    <p>A copy of the configuration behind each generation is kept, which is how going back restores your choices too. A file you have changed by hand since then is left as it is.</p>
  </article>
  <article class="win">
    <h2>Updates, quickly</h2>
    <p>Checking asks GitHub, FlakeHub and the NixOS channels for their newest versions directly: a second or two, and nothing is downloaded. Updating gets the new sources ready as you before it asks for a password. Without that, root would download each one again: in a test, six minutes for one copy of Nixpkgs.</p>
    <p>On Sekka, Updates › Update Sekka fetches the newest Sekka and builds it for your next restart in one action. In a terminal, the same operation is <code>sekka-update</code>.</p>
  </article>
</section>

<section class="tour-step" aria-labelledby="waits">
  <div class="win step-text">
    <h2 id="waits">A window that never waits</h2>
    <p>Nothing slow happens on the interface thread. The configuration, generations and sources are read in a few milliseconds; the store, which can take seconds, fills in when it arrives. Search answers only the newest query, long lists are built a batch at a time between frames, and the package index is made only once Discover is opened.</p>
    <p>The falling snow pauses when the window is in the background. Start Yukimi with <code>YUKIMI_STALLS=50</code> and it reports, on standard error, any moment the interface thread was busy for longer than 50 milliseconds, and what it was doing.</p>
  </div>
  <div class="win step-text">
    <h2>Only one part runs as root</h2>
    <p>Yukimi is Rust throughout, including the parts that read Nix's own store database and edit Nix files without disturbing them. The window runs as you. The helper that changes the configuration, builds, switches, rolls back and collects garbage is the only part that runs as root. It is started through <code>pkexec</code>, checks its own arguments, and works on the configuration it finds itself, not one it is handed.</p>
    <p>What Yukimi installs for everyone goes in a file of its own, <code>yukimi.nix</code>, next to your main configuration, which imports it once. Your own files stay yours.</p>
  </div>
</section>

<section class="win" aria-labelledby="anynixos">
  <h2 id="anynixos">On any NixOS system</h2>
  <p>Yukimi finds the configuration the way <code>nixos-rebuild</code> does: a flake when <code>/etc/nixos/flake.nix</code> exists, following it if it links elsewhere, and channels otherwise. It works without the rest of Sekka:</p>
  <pre class="code"><code>{
  inputs.sekka.url = "github:bitemyapp/sekka/nightly";
  inputs.sekka.inputs.nixpkgs.follows = "nixpkgs";
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        sekka.nixosModules.yukimi
        { programs.yukimi.enable = true; }
      ];
    };
  };
}</code></pre>
  <p>The module installs the app with its polkit policy, so the password prompt says what Yukimi is asking for. A distribution can give Discover its own catalog of applications with <code>programs.yukimi.catalogs</code>.</p>
</section>
"#,
    );
    write!(
        b,
        r#"<section class="tour-step" aria-labelledby="everywhere">
  <div class="win step-text">
    <h2 id="everywhere">The same on every desktop</h2>
    <p>Yukimi is one native app on every Sekka desktop: Aizome, Tatami, Plasma, GNOME and Xfce. In Aizome its commands use Super, as every Aizome app's do; elsewhere they use Ctrl.</p>
    <p>It covers the everyday: installing and removing software, updating, going back and freeing space. Nix can do far more than any window can show, and the configuration stays yours to edit by hand whenever you like.</p>
  </div>
  {tatami}
</section>
"#,
        tatami = site.shot(
            "yukimi.png",
            "Yukimi's overview in Tatami, under Tatami's bar: the machine's name, its package count, how fresh its Nixpkgs is, and how much room the store takes.",
            "Yukimi in Tatami, the whole display, reading the same test VM.",
            "win",
        )?,
    )?;
    Ok(Page {
        path: "/yukimi/",
        title: "Yukimi: see and change your NixOS system".into(),
        description: "Yukimi shows what is installed on a NixOS system and why, finds software, updates, goes back to earlier generations and frees space, without editing Nix by hand.",
        body: b,
    })
}

pub fn status(site: &Site) -> Result<Page> {
    let mut b = String::new();
    write!(
        b,
        r#"<section class="win focus page-head">
  <div>
    <p class="eyebrow">Status</p>
    <h1>How Sekka is made and checked.</h1>
    <p class="lead">Sekka and Aizome are in daily use on many machines, their developer's and their users'. Every night Sekka is built and tested again before it reaches anyone. This page says how each part is checked, what the installer offers, and what Sekka keeps private. It was built from revision {rev}.</p>
  </div>
</section>
"#,
        rev = site.revision_html()
    )?;
    write!(
        b,
        r#"<section class="tiles">
  <article class="win">
    <h2>Nightly builds</h2>
    <p>Automation builds Sekka from <code>main</code> every night and fills the binary cache. A <a href="/download/">nightly ISO</a> joins the downloads once those exact bytes have been installed, booted from disk, logged into and used, and its manifest links that evidence. Installed systems update to each nightly from Yukimi. GitHub's “Source code” archives are not bootable images.</p>
  </article>
  <article class="win">
    <h2>Trying it</h2>
    <p>The image starts a live Aizome session with Genkan; the boot menu also offers an older kernel and a recovery entry that draws Aizome in software. Try the live session on your computer, then install from it. To build an image yourself, see <a href="{REPO}/blob/main/DEVELOPERS.md">DEVELOPERS.md</a>:</p>
    <pre class="code"><code>nix build .#iso --out-link result-iso</code></pre>
  </article>
</section>
"#
    )?;
    b.push_str(
        r#"<section class="win" aria-labelledby="tested">
  <h2 id="tested">What's tested, and how</h2>
  <div class="table-wrap"><table class="evidence">
    <thead><tr><th scope="col">Part</th><th scope="col">How it's checked</th></tr></thead>
    <tbody>
      <tr><th scope="row">Hardware</th><td>Daily use on many machines, the developer's and Sekka's users', among them a Lenovo ThinkPad X1 Carbon Gen 14 with Intel graphics, docked and undocked, and an Acer Predator Helios Neo 14 with an NVIDIA GeForce RTX 4060 Laptop GPU and Intel graphics, switched with PRIME.</td></tr>
      <tr><th scope="row">Installing</th><td>Automated storage tests of the disk layouts. Before an image is published, that exact image is installed in UEFI and BIOS virtual machines, rebooted from disk, logged into, and checked with Yukimi.</td></tr>
      <tr><th scope="row">Updating</th><td>A virtual machine kills the updater's client during live activation, checks the result and that the graphical session survives, rejects an unsafe display-manager change, stages a rollback and boots it.</td></tr>
      <tr><th scope="row">Aizome</th><td>A virtual machine logs into Aizome; its apps are driven headless in tests, with golden images of their main views; frames are measured against a 4 ms budget.</td></tr>
      <tr><th scope="row">Displays and lid</th><td>The display helper runs against a fake Hyprland and a fake lid. Real Hyprland runs in a virtual machine with simulated screens and a simulated lid switch.</td></tr>
      <tr><th scope="row">Keep Awake</th><td>Real logind and polkit inhibition in a virtual machine, including a failing holder, a logind restart and boot persistence.</td></tr>
      <tr><th scope="row">USB wake</th><td>Fixtures for hub paths, unrelated devices and reconnect-like resets.</td></tr>
      <tr><th scope="row">Hyprland crash fix</th><td>Sekka patches Aquamarine's null-connector teardown. A test runs the real library with null connectors and confirms the unpatched one crashes.</td></tr>
    </tbody>
  </table></div>
</section>

<section class="tiles">
  <article class="win">
    <h2>What Genkan installs</h2>
    <ul class="facts">
      <li>ext4, Btrfs or XFS, with UEFI or legacy BIOS boot, on x86_64 PCs.</li>
      <li>Optional LUKS2 encryption for the system, personal files and swap, unlocked at startup with a passphrase of its own, typed into Sekka's boot splash. Boot files stay unencrypted, and TRIM is on.</li>
      <li>RAM-sized swap and system tuning, on by default.</li>
      <li>NVIDIA's driver for Turing and newer GPUs.</li>
      <li>Proprietary software allowed by default, with an opt-out; redistributable firmware may still be proprietary.</li>
    </ul>
  </article>
  <article class="win">
    <h2>How it installs</h2>
    <p>Genkan takes a whole disk: it erases the one you choose, after you've reviewed everything. There's no dual boot or manual partitioning, RAID or LVM, and installing needs an internet connection. Secure Boot needs to be off.</p>
    <p>Disks with native 4 KiB sectors install when the image boots in UEFI mode; legacy BIOS installation needs 512-byte sectors, 512e disks included.</p>
  </article>
</section>

<section class="tiles">
  <article class="win">
    <h2>Updating an installation</h2>
    <p>Open Yukimi › Updates › Update, or run <code>sekka-update</code>. You get Sekka's newest nightly, with NixOS, the applications and the AI tools at the versions it built and tested, and only once all of it is ready to download: nothing is compiled on your computer except NVIDIA's driver, built for your kernel in a few minutes. If space runs short, the update stops before the disk fills up and changes nothing. Sekka applies the update now or at the next restart, whichever is safe. It never restarts or signs you out on its own. Your account, applications, hardware configuration and desktop choices stay in place, and earlier generations stay in Yukimi's History and the boot menu.</p>
    <p>Installations older than this command run it once with <code>nix run github:bitemyapp/sekka/nightly#update</code>.</p>
  </article>
  <article class="win">
    <h2>Privacy</h2>
    <ul class="facts">
      <li>This site sets no cookies and sends no analytics to your browser. Its one script remembers whether you chose Night or Day, in your browser's local storage, and sends it nowhere. Cloudflare serves it and sees requests, as any host would. The download page lists the nightly builds as Cloudflare serves it.</li>
      <li>Tatami's weather asks wttr.in and Open-Meteo; wttr.in locates you by IP address unless you set a place in Setup › Weather.</li>
      <li>Genkan's GitHub step is optional. It reads your public SSH keys and sets your Git identity.</li>
      <li>Sekka's own packages download from cache.sekka.computer, on Cloudflare R2. Only free software is published there.</li>
      <li>The AI applications download from numtide's cache.numtide.com, which Nix asks only for what cache.nixos.org and Sekka's cache don't have.</li>
    </ul>
  </article>
</section>

<section class="win" aria-labelledby="report">
  <h2 id="report">Reporting a problem</h2>
  <p>Please <a href="https://github.com/bitemyapp/sekka/issues">open an issue</a> with what you tried, what happened, and the image version or source revision. Hardware details help with boot and display problems. Check logs for personal information before attaching them.</p>
</section>
"#,
    );
    Ok(Page {
        path: "/status/",
        title: "Sekka's status: how it's made and checked".into(),
        description: "How Sekka is built and tested each night, what its installer offers, and what it keeps private.",
        body: b,
    })
}

pub fn download(_site: &Site) -> Result<Page> {
    Ok(Page {
        path: "/download/",
        title: "Download Sekka's nightly ISO".into(),
        description: "Sekka's nightly ISOs for x86_64 PCs, each published once that exact image has been installed, booted, logged into and used.",
        body: format!(
            r#"<section class="win focus page-head">
  <div>
    <p class="eyebrow">Download</p>
    <h1>Sekka's nightly ISO.</h1>
    <p class="lead">Sekka is built and its binary cache filled each night. An ISO joins the downloads once those exact bytes have passed installation, installed-disk boot, login and desktop testing, and a review of the evidence. The last seven are kept. Each starts a live session with Genkan, the installer, open: Aizome's, or Plasma's in builds from before 27 October.</p>
  </div>
</section>
<section class="win" aria-labelledby="builds">
  <h2 id="builds">Builds</h2>
  <p>Newest first, for x86_64 PCs with UEFI or legacy BIOS boot. Each has its checksum, its manifest (source revision, size, upload time and a link to its test evidence) and the <code>flake.lock</code> it was built from. The files come from Cloudflare R2, at downloads.sekka.computer. GitHub's “Source code” archives are not bootable images.</p>
  <!--builds--><p>The download list is temporarily unavailable. Try again, or check the <a href="{NIGHTLY}/builds.json">download index</a> directly.</p><!--/builds-->
</section>
<section class="tiles">
  <article class="win">
    <h2>Check the download</h2>
    <p>With the build's SHA256SUMS next to its ISO, on Linux:</p>
    <pre class="code"><code>sha256sum -c SHA256SUMS</code></pre>
    <p>On macOS, <code>shasum -a 256 -c SHA256SUMS</code>. In Windows PowerShell, compare the output of <code>Get-FileHash</code> on the ISO with the checksum in the file.</p>
  </article>
  <article class="win">
    <h2>Before you install</h2>
    <p>Write the ISO to a USB drive and start the live session: Aizome runs from it as it will once installed, so you can try your Wi-Fi, graphics, sleep and displays first. Installing erases the disk you choose and needs an internet connection; <a href="/status/">how it installs</a>.</p>
  </article>
</section>
"#
        ),
    })
}

pub fn fonts(site: &Site) -> Result<Page> {
    Ok(Page {
        path: "/fonts/",
        title: "Fonts on sekka.computer".into(),
        description: "The fonts this site serves, and their licenses.",
        body: format!(
            r#"<section class="win focus page-head"><div>
  <p class="eyebrow">Fonts</p>
  <h1>The fonts this site serves.</h1>
  <p class="lead">Both are served from this site, never from a font service, and both are under the SIL Open Font License 1.1.</p>
</div></section>
<section class="tiles">
  <article class="win"><h2 class="type-display">JetBrains Mono</h2><p>Tatami's face, by JetBrains. <a href="{mono}">License</a></p></article>
  <article class="win"><h2>Atkinson Hyperlegible Next</h2><p>By the Braille Institute. <a href="{sans}">License</a></p></article>
</section>
<section class="win"><p>The kanji are outlines drawn from Noto Serif CJK JP, also under the SIL Open Font License.</p></section>
"#,
            mono = site.asset("fonts/JetBrainsMono-OFL.txt")?,
            sans = site.asset("fonts/AtkinsonHyperlegibleNext-OFL.txt")?,
        ),
    })
}

pub fn not_found(site: &Site) -> Result<Page> {
    Ok(Page {
        path: "/404.html",
        title: "Not found: Sekka".into(),
        description: "This page isn't here.",
        body: format!(
            r#"<section class="win focus page-head lost">{crest}<div><h1>Nothing here.</h1><p class="lead">This page doesn't exist, or it moved. <a href="/">Start from the front door.</a></p></div></section>"#,
            crest = site
                .brand
                .inline("sekka-crest-blue.svg", "lost-crest", "The Sekka crest")?
        ),
    })
}
