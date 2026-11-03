# Chanoma 茶の間

Chanoma is Aizome's family television: streaming services, films and shows
from the disk, and games, driven by a game controller alone, with a
grown-up's PIN on what kids may do and for how long. What it does is in
[apps.md](apps.md#chanoma-tv); this is how, and what has been checked.

Turn it on with the Aizome module:

```nix
programs.aizome.chanoma = {
  enable = true;
  # Optional: Google Chrome for DRM-protected services (unfree).
  browser = pkgs.google-chrome;
  # A television: start with every session, and again after a crash.
  autostart = true;
};
# With autostart, the computer comes on into Chanoma.
services.displayManager.autoLogin = { enable = true; user = "tv"; };
```

`enable` installs the package's `chanoma` output (the binary, its desktop
entry and icon, kept out of Aizome's own output so desktops without it
don't list it), the cores (`programs.aizome.chanoma.cores`; snes9x and
Genesis Plus GX, whose licences forbid selling them, only where unfree
packages are allowed) and a PlayStation BIOS
(`programs.aizome.chanoma.bios`: OpenBIOS, below), exports
`CHANOMA_CORES`, `CHANOMA_BIOS` (and `CHANOMA_BROWSER`, for a browser
given) to the session, and sets BlueZ to connect quickly and to
let a controller that forgot the computer pair again without anyone
confirming. `autostart` is a user unit wanted by and ordered after
`aizome-session.target`, which starts once the compositor is ready; it
restarts on failure, and a stop (logging out) keeps a game where it was.
The unit has systemd's short PATH, which is why the browser is passed
explicitly (and Chanoma also looks in NixOS's profiles).

## The program

`apps/chanoma/src`:

| Module | |
|---|---|
| `pad/` | Controllers from `/dev/input`: hot-plug through inotify, the exclusive grab, player slots, mappings, menu navigation with repeat, batteries, rumble |
| `retro/` | The libretro frontend: the C interface, the host (callbacks, environment, core options), video, sound, and a session thread per game |
| `browser.rs` | Services in Chrome: profiles, kiosk flags, the window followed through the compositor's IPC, input through it |
| `devtools.rs` | Services kept to their sites, and Chanoma's keyboard asking and typing on their pages, over the browser's DevTools pipe |
| `services.rs` | The services, the sites each one's pages may go to, and which need Chanoma's keyboard |
| `rules.rs` | The PIN, wrong-PIN lockout, what kids may open, daily limits, bedtime, grants |
| `library.rs` | Systems, cores, games and videos found in folders |
| `folders.rs` | Choosing a folder: the places to start from, a folder's folders and what's in each, what Chanoma would find in one |
| `app/` | The program: screens, input, home shelves, playing, settings, time |
| `ui/` | What's drawn: a 1920 × 1080 stage scaled to the screen |

### The look

Chanoma draws on one canvas rather than with the kit's widgets (a
television wants tiles and a focus that moves by direction, not a
pointer's controls), but in Aizome's vocabulary (`ui/look.rs`): the
theme's palette, so Night, Day, High contrast and the focus colour follow
the desktop's settings; square faces with the 1pt top highlight and
edge; the hard shadow under what floats, where the effects allow; the 2pt
focus ring; the selection's indigo for lit rows and pressed-in choices;
Archivo for words, IBM Plex Mono for clocks and numbers and spaced
capitals over sections; Lucide icons from the kit; Aizome's mark beside
the name. Every length is doubled (`K`): a 1080p television across a
room reads as a laptop's screen at arm's length does at twice the size.
The menus are the kit's titled menus (a dark title strip, engraved rows);
the settings follow Settei's side list; the PIN pad and the one-thing
screens follow the lock screen. The clock follows the menu bar's 24- or
12-hour setting, read at start. Chanoma's keyboard over a page is a
floating panel along the foot of the screen, its keys square faces.

### Controllers

Chanoma reads controllers' event devices itself, on a thread of its own,
rather than through a library, because it **grabs** them (`EVIOCGRAB`):
while a service's page is open, Chrome's Gamepad API would otherwise also
act on presses Chanoma turns into keys. Devices are found with inotify on
`/dev/input`, retried when logind's ACL arrives (`IN_ATTRIB`), and told
apart from keyboards, touchpads and motion sensors by their capabilities.
A controller that comes back (by its Bluetooth address or USB serial)
keeps its player number.

Buttons are named by position (`South` is the bottom face button). The
mapping is, in order: one set up in Chanoma's settings for that model
(`controllers.json`); SDL's community database (the package points
`CHANOMA_GAMECONTROLLERDB` at nixpkgs' `sdl_gamecontrollerdb`), numbered
as SDL numbers a device's buttons, axes and hats; else the kernel's
gamepad layout, with Microsoft's and `xpad`'s left and top buttons swapped
back (they report X, the left one, as `BTN_X`, the same code as
`BTN_NORTH`). Microsoft's Bluetooth controllers, whose right stick is
Z/RZ and triggers brake and gas, map right without the database.

Which button says yes follows the controller (Nintendo's right-hand A,
others' bottom button) unless a grown-up sets it. Menus get a direction
from the arrows or the left stick (with hysteresis), repeating after
420 ms every 130 ms, quickening to 75 ms. Select and Start held for a
second is Home on controllers without one.

Games read the controllers' state from shared atomics the thread keeps
current, not through messages.

### The way home

Wherever Chanoma is, and whatever is in hand, there's a way back to Home:

| | Controller | Keyboard | Mouse |
|---|---|---|---|
| Chanoma's screens | Back, Home | Esc, Home or F1 | the hints along the foot are buttons (*Back*, *Home*), as are a screen's own (*Cancel*, *Back home*) |
| A menu or the PIN pad | Back (on the pad, once its presses are taken back) | Esc | a click beside it |
| A game or a film | Home: its menu | Esc, Home or F1: its menu | moving it brings a bar down from the top, *Home* at its left; a click on the picture is the menu too |
| A page in the browser | Home: the menu over the page | Home or F1, kept from the page | the pointer pushed up to the top of the screen brings the bar down |

A controller always reaches Chanoma, as it reads them itself. A keyboard
and a mouse belong to the window under them, which over a page is the
browser's, so there:

- **Home and F1** (and a remote's Home, `KEY_HOMEPAGE`) are kept for
  Chanoma by the compositor while the browser's window has the keyboard
  (the IPC's `catch-keys`, asked on a subscribed connection once the
  window shows): the page never hears them, and Chanoma hears of each.
  With Ctrl, Alt or ❖ held they go to the page as ever.
- **The menu over the page** takes the keyboard while it shows (an
  exclusive layer surface), so the arrows, Return and Esc answer it;
  put away, the page has the keys again.
- **The bar** comes down when the pointer reaches a strip two points
  high along the top of the screen (a layer surface of its own, nothing
  drawn). It stays while the pointer is in it and goes as the pointer
  moves back down to the page.
- When the page shows, a note over it says how to come back: *Press Home
  on the controller to come back*, or without a controller, *Press Home,
  or move the pointer to the top, to come back*.

### Games

Chanoma loads libretro cores with `dlopen` and drives them itself
(`retro/`), so there is no RetroArch menu for a kid to wander into, and
pausing, saving and leaving are Chanoma's.

- **Environment**: pixel formats (0RGB1555, RGB565 through a 65,536-entry
  table, XRGB8888), core options in all three forms (with Chanoma's
  preferences over a core's defaults: the Game Boy's green palette),
  system and save folders, rumble, input bitmasks, geometry and timing
  changes. Hardware rendering is declined: cores that need OpenGL or
  Vulkan don't run (Nintendo 64, Dreamcast and later; the PlayStation
  plays through PCSX-ReARMed's software renderer).
  Logging is declined too (a C varargs callback).
- **The PlayStation's BIOS**: Sony's can't be shipped, so the module
  offers OpenBIOS (`nix/openbios.nix`), PCSX-Redux's free replacement
  (MIT, from its `nugget` repository), built from source with Nixpkgs'
  MIPS cross compiler, unwrapped (the wrapper adds `-march=mips32r2`,
  where the console's R3000 is MIPS I); it reproduces bit for bit.
  Before a game starts, Chanoma links the files in `$CHANOMA_BIOS` into
  the cores' system folder (`~/.local/share/aizome/chanoma/system`),
  keeping any file of the same name a grown-up put there and clearing
  links to what's gone. PCSX-ReARMed, first for the PlayStation, looks
  for Sony's BIOS by name first (`scph5501.bin` and the like), then takes
  any 512 KiB image that starts as a BIOS does, so a grown-up's dump of
  their own console's BIOS comes first and OpenBIOS after; with neither,
  it imitates one itself (and says so on the screen). Only PCSX-ReARMed
  has been tried with OpenBIOS.
- **Picture**: each frame is converted to XRGB8888 and enlarged by a whole
  number with nearest-neighbour into a memfd of three frames, then shown
  through the window's video sink (`docs/kit.md`, Video); the compositor
  scales the rest, so its smoothing has only a small step left. The
  enlargement is the least that reaches the screen's lines, at most six
  and at most 1,300 lines. The view letterboxes it to the core's aspect.
- **Sound**: resampled linearly to 48 kHz into a lock-free ring, played by
  a PipeWire stream of 512-frame cycles (about 11 ms). Frames are paced by
  the clock; the resampling ratio leans by at most half a per cent toward
  60 ms buffered, absorbing the sound card's drift.
- **Saves**: the core's save RAM is read before the game starts and
  written when it changes (checked every 30 seconds) and at the end.
  Leaving keeps the core's whole state and a PNG of the last frame (the
  *Keep playing* tile); the next start carries on from it.
- Callbacks have no context in libretro, so the running core's host is
  one process-wide pointer: one game at a time. Cores run in Chanoma's
  process; a core that crashes takes Chanoma with it, and the autostart
  service starts it again.

### Services

Each service opens in the first of `google-chrome-stable`,
`google-chrome`, `chromium` and `chromium-browser` on the PATH (or the
grown-ups' choice), with `--kiosk`, a profile of its own under
`$XDG_DATA_HOME/aizome/chanoma/browser/`, no first-run or crash-restore
bubbles, no translation or casting UI, and `--password-store=basic` (no
keyring to unlock). YouTube gets a television's user agent, which serves
its TV interface.

A thread follows the browser through the compositor's IPC: it makes its
window full screen and focuses it, closes any other window of its
process (pop-ups), and reports when the window or the process goes.
Input goes through the IPC's `synthesize`: keys and clicks from button
presses, and pointer motion and scrolling from the sticks on a thread
polling the shared controller state (every 8 ms while a stick leans).
The overlay over the page (the Home menu, the bar, notes) is a layer
surface on the overlay layer, which Aizome draws above a focused
full-screen window, open only while one of them shows; a note lets the
pointer through, and only the menu takes the keyboard (The way home,
above). The strip along the top that brings the bar down is another,
open while the page shows.

### Kept to the service

In kids' mode a service's pages stay on its sites: its address's site
(the host, less a `www.`) and its subdomains, and the few others it
passes through (`Service::sites`: Google's sign-in for YouTube Kids,
which goes through it as it opens, and for YouTube; `max.com` and
`hbomax.com` for Max, whose address now leads to HBO Max). The lists
come from following each address in Google Chrome 156 on 2026-11-01,
signed out. A website a grown-up added stays on its own host's site.
Everything else on the web is outside, including the rest of Google.

The browser starts blank with `--remote-debugging-pipe`, and
`devtools.rs` steers it over the pipe on its descriptors 3 and 4, put
there before it starts. Only Chanoma holds the other ends. There's no
port anything else on the machine could reach, and the browser closes
itself when the pipe does, so Chanoma ending can't leave a page loose.
Over the pipe:

- every page target is auto-attached waiting for the debugger, enables
  `Fetch` for documents, and only then runs; the first is then sent to
  the service, so nothing has loaded unasked;
- a page's own document (its frame id is the page's target id) from
  outside its sites is failed as `Aborted`. Chrome commits nothing for an
  aborted navigation, so the page stays where it was and its history is
  unchanged: links, redirects (each hop is asked about), forms, scripts,
  and a frame's link to the top alike. Chanoma's note says *That's
  outside …*. Frames' own documents load from anywhere (players, sign-in
  boxes);
- behind that, a page whose address ends up outside anyway (a page a
  service worker or the back-forward cache answered, which never asks;
  a file opened from the keyboard) goes back to its last place on its
  sites in its history, or to the service's start, and is left blank if
  it keeps leaving by itself;
- any other page (a tab, a pop-up) is closed as it starts, and
  downloads are refused;
- in grown-up mode nothing is refused, since signing in can pass through
  other sites (Prime Video's through Amazon's). Kids' mode coming back
  keeps the page again, taking it back if it's outside by then. Chanoma
  sets this after every message it handles.

`--disable-blink-features=AutomationControlled` keeps the pipe from
showing in `navigator.webdriver`, which the pipe alone sets; Google's
sign-in refuses a browser that says it's automated. A browser that
doesn't answer on the pipe within 30 seconds (one that isn't Chromium's)
is closed, with a screen saying why. There are two other ways to do it,
and neither fits:
- Chrome's enterprise policies (`URLBlocklist`, `URLAllowlist`) are
  machine-wide, need root, and would fence in the grown-ups' own Chrome
  too.
- An extension: Google Chrome doesn't load one from the command line
  (`--load-extension`; checked on 156, where Chromium still does), and
  DRM services need Google Chrome.

### Chanoma's keyboard

A service's text boxes (a search, a sign-in) get Chanoma's own keyboard
(`ui/keyboard.rs`, `app/typing.rs`) where nothing else types: every
service but YouTube, whose television interface has a keyboard of its
own that the arrows work (`Service::keyboard`), and every website a
grown-up adds.

- **When it comes up.** 300 ms after the confirm button is let go on such
  a page (a click where the pointer is), or after the shoulders move
  between its controls, Chanoma asks the page what has its keyboard. A
  text box chosen that way (within 2 seconds) brings the keyboard up; a
  box moved to with the shoulders brings it up when the confirm button is
  pressed on it, rather than Return. A mouse's clicks don't ask: whoever
  has a mouse has a keyboard, as a rule. Select brings it up by hand, for
  a box the page can't tell of (below).
- **Asking the page** goes over the DevTools pipe (`Ask::Focused`): a
  world of Chanoma's own in the page (`Page.createIsolatedWorld`, made
  again when the page's document changes) evaluates a short probe of
  `document.activeElement`, through open shadow roots and into frames from
  the page's own process. It says the box's label (its `aria-label`,
  placeholder, label, title or name), what's in it and where the caret is,
  or for a password only how long it is; or that a frame from another
  site has the keyboard; or nothing that types. The page's scripts can't
  see that world, and the page's runtime isn't enabled for DevTools:
  `Runtime.enable` is what pages that look for automation notice, and a
  binding the page calls would need it (without it, a binding never reached
  a new document in Chromium or Chrome 156). So the page is asked,
  not listened to: after a choice, after each key typed, and every 700 ms
  while the keyboard is up.
- **Typing** goes over the pipe too, as the browser's own input, to
  whatever has its keyboard, a box in a frame from another site included:
  `Input.insertText` for what's typed, whatever the keyboard layout, and
  `Input.dispatchKeyEvent` for Backspace, Return and the caret's arrows.
  One path keeps them in order.
- **On the keyboard** the arrows or the stick move between keys (the
  sticks no longer move the page's pointer); the confirm button presses
  one; X deletes, Y is a space, the shoulders move the caret, LT is Shift
  (pressed twice, capitals stay), RT the symbols; Start is Enter, which
  sends what's typed and puts the keyboard away; Back, Select or a click
  beside it puts it away too. Keys are buttons for a mouse. Above the keys
  is what's in the box, as the box may be under the keyboard. The overlay
  doesn't take the keyboard, so a real keyboard still types into the page.
- **It goes by itself** when the box lets the keyboard go (a search sent,
  another page), unless it was brought up by hand before a box had it.
  Home brings the page's menu, the keyboard put away.
- **Select and Start** on a page count when they're let go, alone: held
  together, they're Home on a controller without one, and would
  otherwise have brought the keyboard up or pressed Enter (and Start
  played or paused the page) on the way.

### Films and shows

Films play through Utsushi's engine (`aizome-media`'s `Player`), into
the window's second video. Some things come from the engine so that
Chanoma and Utsushi agree:

- **What counts as a video.** `aizome_media::is_video` decides, by
  extension.
- **Where a film carries on.** `resume_point` gives where it was left,
  unless the film is under five minutes, was left in its first thirty
  seconds, or is within half a minute (or the last twentieth) of its end.
- **Where a show starts.** A show's episodes start at the first one not
  `finished`.

**Subtitles and languages.** The engine lists a film's sound and subtitle
tracks: embedded ones, and subtitle files beside it (`film.srt`,
`film.en.srt`). It picks the defaults: a track marked default or forced,
else a file beside it.

- **The menus.** The film's menu (Back) adds *Subtitles* where there are
  any, and *Language* where there's more than one sound track. Each
  opens a menu of the tracks, with the focus on the one playing; Back
  returns to the film's menu.
- **What's kept.** A choice is kept in `recent.json` for the show (its
  folder) or the film (its file). It's matched by language where the
  track names one, so the next episode's English is found wherever it
  sits (`en` and `eng` alike), and by place otherwise. A show's next
  episode, and the next time, open with it.

**Drawing subtitles.** The engine sends cues as it reads them, and
`aizome_media::Cues` keeps them, as it does for Utsushi. A timer wakes
Chanoma whenever what shows changes.

- Words are drawn on the stage canvas, white on a dark band behind each
  line, at 4.5% of the picture's height.
- They go in the letterbox under the picture where they fit, so nothing
  covers it, and over its foot otherwise. A cue placed at the top goes
  at the top.
- They stay above the film's bar while it shows.
- A disc's picture subtitles are drawn where they were placed.
- While subtitles show, the stage is over the video (`over_video`), as
  it is for the bar.

**The desktop's media controls.** Chanoma serves MPRIS as
`org.mpris.MediaPlayer2.chanoma`, through `aizome-mpris` as Utsushi does.

- A keyboard's or a remote's media keys, and the menu bar's media
  widget, can play, pause, seek, skip episodes and stop the film.
- The volume they set is the film's.
- They can't ask Chanoma to open anything, to change speed or to quit
  (leaving takes the PIN). Raise brings Chanoma forward.
- Between films, Chanoma says nothing is playing.

### Rules

The PIN is four to eight presses of the controller's buttons, as a
console's passcode is (`ui/pinpad.rs`). Each press is the same dot in
its cell, and nothing in the legend lights, so someone watching sees that
a button went but not which. The legend is the buttons a PIN is made of:
the four arrows, the shoulders, and the top and left face buttons. They're
named by place, as every button is here, so a PIN made on one controller
is the same presses on any other, whoever made it; the legend draws each
in the prints of the controller pressed last, with a mark under the face
buttons saying which place they are (Xbox's Y is Nintendo's X). Left out:
the yes and no buttons, which Nintendo's controllers swap (they're Done
and Delete), the triggers, which some pads lack and others press only
part way, and the sticks. The arrows count as presses of the direction
pad only, once each, never the stick or a press held. Delete (the no
button, Backspace, or the pad's Delete clicked) takes the last press back,
and with none, puts the pad away. Each of the legend's buttons is
numbered, 1 to 8, in its corner: a PIN is kept as those digits (1 is
Up), so a PIN made of digits 1 to 8 before PINs were buttons is still
the same, pressed by the numbers; one with a 9 or a 0 needs
`--reset-pin`. The legend never changes as presses go in, so the numbers
give nothing away. A keyboard presses them with those digits, or with the
arrows, Page Up and Page Down, Tab and Space, the keys Chanoma's hints
show for those buttons; the legend shows those keys when a keyboard was
last used.

It's kept as PBKDF2-HMAC-SHA-256 with a random salt and 150,000
rounds (about a tenth of a second, checked off the interface thread).
Three wrong PINs are free; then the pad waits 30 seconds, doubling each
time up to 15 minutes, and the count is kept in `lockout.json`. This keeps
out kids with a controller; it can't resist someone who can run code as
the TV's user (who can read the hash, or `aizome-chanoma --reset-pin`).

Time counts while a game, a service or a film is open in kids' mode,
into `usage.json` (written every 30 seconds while it counts). The gate
is open with the least of today's limit (plus grants) and the time until
bedtime left. A grant tops today up from what's been used, so a grant
after time ran out is all play, and lifts bedtime that long.

### Choosing folders

The kit's file panel wants a pointer and a keyboard, so the settings'
*Add a games folder* and *Add a videos folder* (and Home's *Add games*)
open a picker of Chanoma's own (`folders.rs`, `app/picker.rs`,
`ui/picker.rs`), laid out as the settings are:

- **Places** down the left: home, the usual folders in it that exist
  (`Games`, `ROMs`, `Videos`, `Movies`, `Downloads`), each drive under
  `/run/media/$USER`, `/media/$USER`, `/media` and `/mnt`, and the whole
  computer. An empty folder there is a drive's mount point while it's
  out, and is left out. The places are read again every second while the
  picker shows, so a stick plugged in appears. A drive pulled out from
  under the folder showing goes back home.
- **The list** on the right: where the folder is, in words ("Home ›
  ROMs › snes"); then the choice, *Use this folder*, with what Chanoma
  would find there; then the folder's own folders, hidden ones left out,
  in natural order, each with a word on what's directly in it ("12 games
  · 3 folders").
- **What's found** is the library's own reading (`scan_games` or
  `scan_videos`, through the files index, below), so the
  count is what Home will show, cores included ("38 games for 4 systems,
  and 2 without an emulator").
- **Off the interface thread:** folders are read there, one reading per
  folder shown. Each answer carries its reading's number, and an older
  one is dropped. A folder of more than 400 folders isn't looked into
  folder by folder.

The controller:

| Button | In the list | Among the places |
|---|---|---|
| Up, down | move | move (the list follows) |
| Confirm, right | open the folder (the first row: use this one) | into the list |
| Back | up a folder, the focus on the one left (from `/`, to the places) | put the picker away |
| Left | to the places | |
| Top face button (Y, △; Tab) or Start | use the folder showing | use the folder showing |
| Shoulders | a page up or down | |

Choosing returns to the settings with the focus on *Add a … folder*, not
on the folder just added, so pressing again doesn't remove it.

### A big collection

A household's games can be thousands of files over a dozen systems, on a
drive beside its backups. Games' settings handle that a system at a time
(`collection.rs`, `app/grownups.rs`):

- **Read through the files index** (`aizome-index`,
  `docs/files-index.md`): games and videos folders are read to any depth,
  with no limit on files (before, five folders deep and 20,000 files,
  which on a drive whose backups come first alphabetically found only the
  backups). Home shows what the index knew at once, then looks again in
  the background and shows that (`Msg::Scanned`); on a warm drive the
  second look takes a fraction of a second.
  Hidden files are left out: a Mac copying to a FAT drive leaves a
  4 KB `._Game.sfc` beside every file.
- **Zipped games** (`zip.rs`), as cartridge sets come: a zip in a folder
  named for a system that isn't a disc's is one of its games (a zip
  elsewhere could be anything's, and isn't opened to see). A core takes a
  file or its bytes, never a zip (RetroArch unpacks them itself), so the
  game thread unpacks it before the core starts, into
  `$XDG_CACHE_HOME/aizome/chanoma/unzipped/<crc>/`, as the file it
  holds (cores go by its extension): the file with one of the system's
  extensions, else the largest. Stored and deflated files are read, the
  CRC checked; the eight played last stay unpacked. Saves and states go
  by the game's key, the zip's path, as for any game.
- **Find games on your drives…** looks through home and each drive
  plugged in (the picker's places) and lists the folders of games it
  finds, by place and system, each with how many and where ("2 games in
  roms › snes"). It shows what the index knew first, then each place as
  it's read, with how many folders so far. A system's games count at the
  nearest folder named for it (`roms/psx`, where each disc has a folder of
  its own); others at their own folder if it holds three or more, so a
  stray `.bin` in a backup isn't a collection. Each is a switch: on adds
  it as a games folder, off takes it away. Files are counted by name, so
  a disc both in a playlist and on its own counts twice here (not on
  Home).
- **Which games show**, before kids' rules, from the tags No-Intro's and
  Redump's names carry (`(USA)`, `(Rev 1)`, `(Beta)`, `[!]`), so a
  well-named set needs no database:
  - **Systems**: a page of every system with games found, each a switch,
    so a whole system goes with one press (`systems_off`).
  - **Hide demos, betas and prototypes** (on unless turned off;
    `hide_variants`): demos, betas, prototypes, samples, kiosk and test
    discs, promos, pirates, and GoodTools' bad dumps, hacks, trainers,
    translations and the like. A BIOS is never a game.
  - **One of each game** (on unless turned off; `one_of_each`): a game's
    regions and revisions show as one, the **Region first**'s (`region`;
    from the computer's language unless chosen: `ja` or `_JP` is Japan;
    the Americas or none, the USA; others, Europe), then the World's, then
    any; of those, the latest revision, then the game as sold. What isn't
    a region, a language, a revision or a date stays part of the game:
    `(Disc 2)` and `(Arcade Mode)` are games of their own.
- **Which games kids see**: *New games*, *Allow all N new games*, then a
  row per system ("12 of 340", for kids) opening its games: *Let kids play
  all N* and *Keep all N from kids*, then each game shown, a switch, with
  its regions, *a revision*, and *New* if it waits for a grown-up. The
  shoulder buttons (Page Up and Down) jump down the list a letter at a
  time, a leading "The" ignored. Back or Left returns to Games, on the
  row the page was opened from.

What's hidden by these isn't forgotten: turning a switch back shows it
with whatever kids' choice it had.

### What's known of a game

The module installs RetroArch's game databases for Chanoma's systems
(`programs.aizome.chanoma.metadata`, the `chanoma-metadata` package,
`nix/chanoma-metadata.nix`): the `rdb/` files of libretro-database (CC
BY-SA 4.0, credited in the settings' Chanoma section), 17 files, 21 MB,
fetched one by one at a pinned commit (the repository is close to a
gigabyte), unchanged. `metadata.rs` reads them (`RARCHDB\0`, an offset,
then a MessagePack map a game until a nil) when the library is read, and
keeps what it says of the games found: the year, the genre, how many can
play, who made it.

- **By name first**: a set named as No-Intro and Redump name them has the
  databases' names (a zip's name is its game's).
- **Else by what's in the file**, worked out on the refreshing pass and
  kept as a fact in the files index (forgotten when the file changes): a
  zip's CRCs from its central directory (nothing unpacked); a
  cartridge's CRC, and an NES game's without its 16-byte header (64 MiB at
  most); for a PlayStation disc, the serial its `SYSTEM.CNF` boots
  (`BOOT = cdrom:\SCUS_949.00;1` is `SCUS-94900`), read from the first
  track named by its cue sheet (or a playlist's first disc), in
  2,352-, 2,336- or 2,048-byte sectors. CHD and PBP discs aren't read.
- A game named twice in a database (a release per disc or revision) takes
  what any of its entries says.

What it's for:

- **Under the focused game** on Home and in the grid, after its system:
  "1996 · Action · 1 player".
- **For 2 players**: with two controllers or more connected, Home shows a
  shelf of the games that many can play, before Games (and the grid a tab
  of them): those the database says take that many, on a system Chanoma
  gives that many controllers. Home's focus stays on its shelf as the
  shelf comes and goes (`home_marks`).
- **The grown-ups' list of a system's games** says the same beside each
  game's regions.

Ratings aren't used: libretro-database's are ESRB's, and few (6 of the
PlayStation's 13,512 entries have one); there's no openly licensed source
of review scores.

## Checking it

```sh
cargo test -p aizome-chanoma                                   # unit and headless
CHANOMA_SHOTS=/tmp/shots cargo test -p aizome-chanoma --test chanoma   # with pictures
CHANOMA_TEST_CORES=<folder of *_libretro.so> \
  cargo test -p aizome-chanoma --test cores -- --ignored --nocapture
CHANOMA_TEST_BROWSER=<chromium or google-chrome-stable> \
  cargo test -p aizome-chanoma --test devtools                 # a page kept, in a real browser
cargo run --release -p aizome-chanoma --example library -- FOLDER… \
  [--metadata DIR] [--db FILE]                                 # what's found, and known, timed
```

- **Unit tests** cover the PIN hash (against RFC 7914's PBKDF2 vector),
  lockout, limits, bedtime across midnight, grants, ioctl numbers,
  controller capabilities, SDL database numbering (a Microsoft Bluetooth
  controller's entry), the kernel layout for Microsoft's and Sony's
  drivers, events into buttons and axes, navigation repeats and stick
  hysteresis, the sound ring and resampler, pixel conversion and
  enlargement, core options, game and video scanning (systems by folder
  and extension, cue sheets' tracks, box art, shows) through the files
  index, layouts, and a language chosen found again in the next episode
  (by name, `en` as `eng`; by place without one; none rather than
  another language), and the sites a page may be on (subdomains; not
  look-alikes, a user before the host, the rest of Google, or what isn't
  the web), which services get Chanoma's keyboard, the PIN's buttons
  (none of yes, no, the triggers or Start) and where clicks on its pad
  land, and the keyboard's rows, moving between its keys, Shift and its
  symbols. What plays, where a film carries on and the subtitles' cues
  are `aizome-media`'s, tested there; serving the media controls is
  `aizome-mpris`'s, tested there against a private `dbus-daemon` (what a
  player says, and which asks reach it).
- **Headless** (`tests/chanoma.rs`, also in the package's check): the
  first setup, its PIN pressed on the controller (a slip taken back with
  B; the stick, a trigger, Start and Select pressing nothing) and again on
  the keyboard; Home for kids and grown-ups (the PIN typed by its numbers on
  a keyboard, 9 and 0 typing nothing); a PIN made on an Xbox
  controller pressed on a Nintendo one, its B taking a press back; wrong
  PINs and their wait, surviving a restart; a game played through a
  libretro core written in the test (frames reaching the window's video
  sink enlarged, the controller reaching the core, pausing under the Home
  menu, its state and picture kept, *Keep playing*, carrying on); time
  running out mid-game, with the warning, the game kept, the closed
  screen, and a grown-up's grant; bedtime; a games folder chosen with the
  controller alone (the places, a folder's folders with what's in each,
  what Chanoma finds there, up again to the folder left, a drive among
  the places, the top button using it, the settings back on *Add a games
  folder*, the new game read, and the picker put away unchanged); the way
  home without a controller, over a game (Esc, a click beside the menu,
  the bar the pointer brings and its *Home*, a click on the picture) and
  over a page, with a stand-in browser (the *Home* hint clicked, Esc, the
  note saying how to come back, the strip along the top bringing the bar,
  the menu taking the keyboard and giving it back, the bar going as the
  pointer returns to the page, and the compositor's word of Home on the
  keyboard leading home; kept to its service in kids' mode, a link out
  said to be outside; a browser that can't be kept ending on a screen
  saying so); Chanoma's keyboard over a page, with the page's answers
  told by hand (a box chosen with A bringing it, the stick moving between
  keys, Shift for a letter, what's in the box shown, a key clicked, B
  putting it away; Select bringing it by hand and the box letting it go;
  Select and Start together doing neither; Start as Enter; a box moved
  to with RB; Home; and none on YouTube's television interface); a film, the engine's test card with a SubRip
  file beside it (its subtitles showing as it plays, in italics where the
  file says; the film's menu offering them and no language for one sound
  track; the subtitles' menu on the track showing, Back to the film's
  menu, Off; kept, and the film opening again without them; the media
  controls' play and stop). A big collection: a No-Intro-named set on a
  games folder and a drive plugged in beside a backup, one of each game
  shown (no beta, no BIOS), the Systems page turning the Game Boy off with
  one switch, every region and then the beta shown and hidden again, a
  system's games all let in for kids and all kept out, the shoulders
  jumping a letter, and *Find games on your drives* listing the drive's
  SNES folder where it's named (not the backup's stray `.bin`) and adding
  it, its games joining one of each. What the databases know: a zipped
  game and a renamed one (found by its CRC) beside one found by name,
  each saying its year, genre and players; a second controller bringing
  *For 2 players* before Games with the focus kept on Games, the zipped
  game playing through the stand-in core from its unpacked file, and the
  shelf going with the controller. The unit tests add where clicks land:
  the hints, the bar's *Home* in a 16:10 window, beside a menu; the
  names' tags (regions, revisions, variants, a BIOS, discs and modes kept
  apart, dates) and the region a language puts first; Mac resource forks
  not taken for games. Zips are read stored and deflated, checked by
  their CRC (a damaged one refused), unpacked, and the oldest cleared; a
  database is read and games found in it by name, CRC and serial; a disc
  image (written by the test) says its serial, from a cue sheet and a
  playlist.
- **Real cores** (`tests/cores.rs`, opt-in): every system's cores from
  nixpkgs load and list the extensions Chanoma sends them; gambatte,
  SameBoy and mGBA play a Game Boy game the test writes, whose screen
  turns black while a direction is held, through whole sessions with the
  controller state, and carry on from their saved state. On 2026-10-31
  with nixpkgs' cores of that date: all three passed (mean brightness
  249–252 untouched, 3 with Right held).
- **PlayStation games with OpenBIOS** (`tests/cores.rs`'s
  `playstation_games_start_with_a_bios`, opt-in, with
  `CHANOMA_TEST_PSX_GAMES`, `_SYSTEM`, `_OUT` and `_STAGES`): each game
  played through PCSX-ReARMed with a given system folder, a picture kept
  every eight seconds and its brightness printed, Start and Cross pressed
  from the third picture on. On 2026-11-02, forty US games from a
  household's own library (cue sheets on a USB drive), with nixpkgs'
  PCSX-ReARMed, once with OpenBIOS and once with a Sony BIOS dumped from
  a console (SCPH-5501), compared picture by picture: all forty started
  under both and reached the same screens (titles, menus, memory card
  checks and file screens, name entry, races and fights) with nothing
  hung or drawn wrong under OpenBIOS. Some ran behind under it, by up to
  a picture (Gran Turismo 2, Tomb Raider, Twisted Metal 2); Mega Man
  Legends' first load was slower and caught up in a two-minute run. The
  games: Crash Bandicoot (and Warped), CTR, Castlevania: Symphony of the
  Night, Final Fantasy VII, Final Fantasy Tactics, Metal Gear Solid, Gran
  Turismo 2, Tony Hawk's Pro Skater (and 2), Resident Evil Director's
  Cut, Silent Hill, Tekken (and 3), Ridge Racer, Wipeout, Mega Man X4,
  Mega Man Legends, Tomb Raider (and II), Sesame Street's Elmo's Letter
  Adventure and Elmo's Number Journey, Chrono Cross, Legend of Mana,
  Jumping Flash!, Street Fighter Alpha 2 and 3, Spider-Man, Twisted
  Metal 2, Syphon Filter, Driver, Dino Crisis, Wild Arms, Legend of
  Legaia, Persona, Bust-A-Move 4, LEGO Racers, Frogger, Metal Slug X and
  Lunar: Silver Star Story Complete.
- **A real browser** (`tests/devtools.rs`, opt-in): the browser started
  and kept as Chanoma does, with no display (`--ozone-platform=headless`),
  on a site of 127.0.0.1 with elsewhere at localhost, served by the test,
  which hears every request. A page tries a script, a server redirect, a
  form and a pop-up out, and stays after each; elsewhere is never asked,
  but for a frame inside the page, which loads; `navigator.webdriver` is
  false. Then grown-up mode lets the page go, kids' mode takes it back
  and refuses it again, and the pipe closing ends the browser. On
  2026-11-01 it passed with nixpkgs' Chromium 156.0.8078.73 and Google
  Chrome 156.0.8078.79 (7.7 s each), and failed as it should with the
  refusing turned off. A second page has boxes for Chanoma's keyboard:
  asked, it says nothing types, then the search box the page gives the
  keyboard (its label, empty); typing `héllo`, Backspace, the caret left
  and `X` leave `hélXl` with the caret before the last letter; Return
  sends it; the password box after says only how long it is; and a frame
  from another site says only that it's a frame. Nothing is added to the
  page's own world meanwhile. On 2026-11-01 both passed with the same two
  browsers (7.8 s and 7.9 s).
- **In a VM** (`nix build .#aizome-checks.chanoma`): the packaged app in an
  Aizome session, started by the module's autostart, with a uinput
  controller that looks like Microsoft's Bluetooth one and nothing else.
  It checks:
  - the controller found and held (another reader can't grab it);
  - a game opened from Home by the controller, and its picture, as the
    compositor captures it, darkening while Right is held (on
    2026-10-31: 103, then 39, then 103 again), and OpenBIOS linked into
    the cores' system folder as it starts; the game is zipped, and is
    unpacked for gambatte, and the games' databases the module installs
    are read (a copy of it named as the Game Boy's database names a game
    is found there);
  - the Home button pausing it under the menu, and stopping keeping its
    state and picture;
  - the controller unplugged and back as player 1;
  - a page added as a website opened in Chromium full screen, where the
    confirm button clicks, the arrows are keys and the stick moves the
    pointer, then the overlay's menu over the page and the way home;
  - the same page without the controller: Home on the keyboard opening
    the menu without the page hearing it (nor Esc, which puts the menu
    away), the page having the keys again after, F1 and the menu's keys
    going home; then the pointer pushed to the top bringing the bar, its
    *Home* clicked, and the menu's *Go home* clicked;
  - the page kept to its site (127.0.0.1): its link to localhost clicked
    and a script going there both refused, the page as it was and the
    server never asked, the site's own other page opening, and the page
    not told the browser is steered;
  - the page's search box clicked with A, bringing Chanoma's keyboard; A
    typing `q` into the page and X taking it back; the arrows to `h` and
    `i`, typed; Start sending `hi` (the server asked for `/?q=hi`) and
    putting the keyboard away;
  - stopping the unit mid-game, as logging out does, keeping it;
  - Chanoma started again, the PIN pressed on the controller from Home's
    grown-ups' door: the pad the same, as the compositor captures it,
    after one press of Down as after one of Up (the first taken back
    with B), and Down, Right, RB, X opening grown-up mode.

  On 2026-11-01 every step passed (the script in 65.9 s), keeping three
  screenshots in its output: the keyboard over the page, the PIN pad after
  a press, and the page left where it was after a link elsewhere. On
  2026-11-02 it passed again with the BIOS step and the numbered PIN
  legend (65.3 s), the package's check phase running Chanoma's 60 unit
  tests and 11 flows. Later that day it passed with the game zipped
  (unpacked for gambatte; 103, then 39, then 103) and the databases read
  (66.6 s), the check phase running Chanoma's 65 unit tests and 13 flows.
- **Live** on the X1 Carbon, under a headless Aizome compositor of its
  own beside the desktop (release build, gambatte, generated videos):
  Home, a game shown through the compositor at the Game Boy's shape, the
  pause menu over its picture, *Keep playing* with the picture of where it
  was left, a show's episodes playing on by themselves and Home after,
  PBS KIDS loading in kiosk Chrome and Chanoma coming back when it
  closed, and SIGTERM mid-game keeping the state.
- **A household's drive** (the `library` example, read only, its files
  index in a scratch database): on 2026-11-02, the `roms` folder of a
  USB drive (exFAT; folders for 214 systems, 89,803 files, cartridge sets
  zipped) with the free cores (no Mega Drive or Game Gear core, whose
  1,233 games were counted, not listed) and the databases of
  `chanoma-metadata`: 6,509 games to play (before zips were read, about
  360), 5,831 of them known to the databases (4,219 by name, 1,612 by
  their zips' CRCs), the first look in 7.7 s with the drive recently
  read, the second in 0.4 s. Before hidden files were left out, the same
  drive showed 201 Mac resource forks (`._Game.sfc`) as games.

Not yet checked: a physical controller (Bluetooth pairing, and the
mappings of real pads: Xbox, PlayStation, Switch Pro, 8BitDo), the
Bluetooth pairing screen against BlueZ, DRM services (Netflix and the
like) in Google Chrome, sound, the ISO, or a television. These need doing
before calling any of it tested on hardware.

## Limits

- The services' sites come from their addresses followed signed out.
  Signed in, a service may pass through a site that isn't on its list,
  which would show as *That's outside …* where it shouldn't (until it's
  added to `Service::sites`); no service has been tried signed in with
  the lock.
- In grown-up mode a page goes anywhere, and grown-up mode doesn't lock
  itself under a page: a page handed to a kid in grown-up mode isn't
  kept until kids' mode comes back.
- Links to other programs (`mailto:`, an app's own scheme) aren't web
  pages, so this doesn't refuse them; what Chrome does with one in kiosk
  mode hasn't been tried.
- Chanoma's keyboard comes up by itself only for a box in the page or a
  frame from its own process: a box in a frame from another site (Apple's
  sign-in is one), or behind a closed shadow root, takes Select (typing
  reaches it all the same, but what's in it doesn't show). Its keys are
  English letters and ASCII symbols, with no accents or other scripts,
  and no input method for Japanese or Chinese. A page's own suggestions
  under its box are reached by putting the keyboard away. It hasn't been
  tried on a service's real sign-in or search.
- A PIN needs the arrows, both shoulders and the top and left face
  buttons: a controller without them (an NES-style pad) can't press
  every PIN.
- DRM-protected services need Google Chrome; nixpkgs' Chromium has no
  Widevine.
- No hardware-rendered cores; no disc swapping for multi-disc games; no
  touch screens (the DS).
- OpenBIOS has been checked with PCSX-ReARMed on forty games' first
  minute: not saving to a memory card and loading again, the BIOS's own
  screens (the memory card manager, the CD player), games from Europe or
  Japan, or long play. Some games load a few seconds slower with it than
  with Sony's.
- Which games show is read from their files' names: a set not named as
  No-Intro or Redump name them (or a game renamed by hand) shows each
  file as a game of its own, and a name that doesn't say its region is
  ranked after those that do.
- Zips are read only in a folder named for their system, stored or
  deflated, under 4 GiB; 7-Zip and RAR aren't read, nor zipped discs.
  Only the NES's header is taken off for a CRC: a renamed game in
  another headered format (a Lynx `.lnx`) may not be found by it.
- The databases know about nine in ten of a well-kept set and fewer of
  the PlayStation's games (libretro-database has a year, genre and
  players for about seven in ten of its entries); nothing is said of a
  game they don't know. A renamed set's files are read once, on the first
  refreshing look after they appear (minutes for thousands of games on a
  slow drive, less for zips, whose CRCs are in their directories). No
  ratings: the databases' are ESRB's, and rare.
- *For N players* trusts the databases' player counts, which give the
  most a game takes (simultaneous or taking turns isn't said), and the
  controllers connected, not who's holding them.
- *Find games on your drives* reads every folder of home and each drive
  each time it's asked; a drive just plugged in takes minutes on a slow
  USB stick or exFAT drive (`docs/files-index.md`), showing what it
  found as it goes. Nothing finds games by itself when a drive appears.
- Adding a website needs a keyboard and a mouse: its address is typed
  in a dialog of the kit's, where Chanoma's keyboard doesn't come.
  Choosing folders doesn't: see Choosing folders above.
- Subtitles lose ASS styling (italics and top placement are kept), as in
  Utsushi. A subtitle file beside a film with no language in its name is
  called *Track 1*. There's no household language to start every film
  in: a choice is kept per show or film. Choosing a language hasn't been
  tried on a film with several sound tracks; the engine's test media
  have one each, so the matching is checked in unit tests only.
