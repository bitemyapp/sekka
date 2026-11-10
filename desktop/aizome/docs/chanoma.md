# Chanoma 茶の間

Chanoma is Aizome's family television: streaming services, films and shows
from the disk, and games, driven by a game controller alone, with a
grown-up's PIN on what kids may do and for how long. What it does is in
[apps.md](apps.md#chanoma-tv); this is how, and what has been checked.

Turn it on with the Aizome module:

```nix
programs.aizome.chanoma = {
  enable = true;
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
packages are allowed; PCSX2 only on x86-64) and a PlayStation BIOS
(`programs.aizome.chanoma.bios`: OpenBIOS, below), exports
`CHANOMA_CORES`, `CHANOMA_BIOS` and `CHANOMA_BROWSER` to the session,
installs the browser (`programs.aizome.chanoma.browser`: Google Chrome
where unfree packages are allowed, as the services that protect their
shows play only in it; none otherwise, and Chanoma looks on the PATH),
keeps the TV account's own systemd to what it needs
(`programs.aizome.chanoma.tvUnits`, under The TV account), and sets
BlueZ to connect quickly and to let a controller that forgot the
computer pair again without anyone confirming. `autostart` is a user unit wanted by and ordered after
`aizome-session.target`, which starts once the compositor is ready; it
restarts on failure, and a stop (logging out) keeps a game where it was.
The unit has systemd's short PATH, which is why the browser is passed
explicitly (and Chanoma also looks in NixOS's profiles).

## The program

`apps/chanoma/src`:

| Module | |
|---|---|
| `pad/` | Controllers from `/dev/input`: hot-plug through inotify, the exclusive grab, player slots, mappings, menu navigation with repeat, batteries, rumble |
| `retro/` | The libretro frontend: the C interface, the host (callbacks, environment, core options), the cores' log (`log.c`, in C for printf's arguments), the calls into cores with their C++ exceptions caught (`guard.cpp`), video, OpenGL for the cores that draw with it, sound, the consoles' BIOS images, and a session thread per game |
| `browser.rs` | Services in Chrome: profiles, kiosk flags, the window followed through the compositor's IPC, input through it |
| `devtools.rs` | Services kept to their sites, and Chanoma's keyboard asking and typing on their pages, over the browser's DevTools pipe |
| `services.rs` | The services, the sites each one's pages may go to, and which need Chanoma's keyboard |
| `rules.rs` | The PIN, wrong-PIN lockout, what kids may open, daily limits, bedtime, grants |
| `library.rs` | Systems, cores, games and videos found in the folders, each read once, and what each holds |
| `disc.rs` | Which system's a disc is, from what's on it (cue sheets, ISOs, CHDs, CSOs) |
| `folders.rs` | Adding a folder: the places to start from, the folders that look like a collection, a folder's folders and what's directly in each, what the files index knows of one |
| `art.rs` | Games' pictures beyond those beside them: what `gamelist.xml` names, and box art fetched from libretro-thumbnails and kept |
| `watch.rs` | Chanoma starting again after a crash, saying what happened |
| `app/` | The program: screens, input, home shelves, playing, settings, time, search, a forgotten PIN |
| `ui/` | What's drawn: a 1920 × 1080 stage scaled to the screen |

### Starting

From its first frame Chanoma shows its name (`app/splash.rs`): Aizome's
mark, "Chanoma" and "Getting your games ready", until Home has its
games, so a television never shows nothing and then a Home that fills
in. Home comes when the first look at the folders, at what the files
index knows, is in (tens of milliseconds for thousands of games) and no
folder's drive is being opened ("Opening the drive Extreme Pro": its
games come once it's open; a drive UDisks hasn't said anything of yet is
waited for 3 s at most). It's never up longer than 10 s, and buttons do
nothing on it. A first start, with nothing in the index yet, goes
straight to Home, which shows the folders being read.

How long it took goes to the journal on its own (`chanoma: Home, with N
games, T after starting`: the `aizome_chanoma::start` target), as does
the GPU a game drawn with OpenGL draws on and whether its frames are
shared with the compositor (`aizome_chanoma::retro::gl`). The rest of
Chanoma's info stays out of it, what's typed over a page among it. On a laptop docked to the television with its lid closed, the
compositor knows the lid as it starts ([compositor.md](compositor.md),
"The lid"), so Chanoma opens on the television rather than the panel
behind the lid.

### After a crash

Nobody should need Ctrl+Alt+Delete to get the television back
(`watch.rs`). `aizome-chanoma` watches itself: the process started runs
Chanoma as its child and waits. When the child dies of a crash (SIGSEGV,
SIGBUS, SIGILL, SIGFPE, SIGABRT, SIGSYS or SIGTRAP: a game's core
faulting, say), or of a panic on its own thread, the watcher starts it
again, and it opens on a page saying so ("Chanoma started again: It
closed suddenly while Flycast was playing Crazy Taxi: the game's player
crashed. Play since the game last saved may be lost."), whose button goes
Home. Leaving, logging out, a signal to stop (passed on to the child) and
a force quit (SIGKILL) end the watcher with it; three crashes in two
minutes end it rather than loop. The journal says each crash, its signal
and what was being played (`chanoma: crashed (SIGSEGV) [Flycast, Crazy
Taxi]; starting it again`).

What was being played is in a note the watcher gives the child
(`$XDG_RUNTIME_DIR/aizome-chanoma-PID.note`): a game's session writes its
core and title there as the core loads and empties it once the game's
put away. A crash's handler adds the signal, and leaves no core dump
(`PR_SET_DUMPABLE`), so the window goes at once: a dump of a gigabyte had
held a frozen picture on the screen for twenty seconds.
`CHANOMA_CORE_DUMPS=1` keeps them, for looking into one. Cores that
handle faults of their own (Flycast's and PCSX2's fast memory) put their
handlers before Chanoma's and hand it what isn't theirs.

A core that throws a C++ exception out of a call (Play! does when it
can't boot a disc) doesn't crash Chanoma: the calls into a core go
through a few lines of C++ (`retro/guard.cpp`) that catch it, with no
Rust between the core and the catch, as Rust can't unwind a foreign
exception. The game ends, nothing of it kept since it last saved, on a
page saying what the exception said ("This game stopped").

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

**Nothing runs off the screen.** Whatever can outgrow the screen scrolls
to keep the focus in sight:
- Home's shelves, the focused tile with a neighbour showing; each shelf
  that holds more than it shows starts with the way to all of it (*All
  games*, *Everything*, *All of them*);
- the grids, the settings' rows and the picker's lists;
- the Games screen's tabs (`Grid::tab_rects`), the chosen system whole,
  in the middle as near as the ends allow, the cut end fading with an
  arrow in the margin (a tab can be clicked), and named beside the title
  too ("PlayStation · 267");
- menus longer than eight items (a film's many subtitle tracks), the
  focus in the middle, arrows above and below.

Text that can be long is cut with an ellipsis where it's drawn:
- titles, and the notes after them, stop short of the header's right
  side (`ui::head_title`);
- toasts and the foot's banner keep within the margins, the banner short
  of the hints;
- a focused tile's name under it stops at the margin;
- menu items stop short of their button.

`ui::home`'s and `ui::menu`'s tests put every tab and every item of a long
menu in focus and check it's on the screen; the flows go along every
system's tab.

**A row says what pressing it does** (`ui::settings`), at its right and
in the confirm button's hint along the foot:
- a chevron goes into a page (a system's games, a folder, *Your folders*,
  *Search every drive for games*), with its count before it where it has
  one ("3 of 5"); right goes in as well, the way it points, as it goes
  into a folder in the picker, and left and Back come out to the row it
  was opened from (*Your folders* back to Games, or Films and shows);
- a switch is switched ("Turn on", "Turn off"), and solid arrows either
  side of a choice (◀ No limit ▶) change it, left and right or "Change"
  stepping on: not chevrons, so a choice never looks like a page;
- an action has its own sign, as the picker's plus has: a plus to add (a
  folder, a website, more time today), arrows round to look or try again,
  a bin to remove, an eye or a crossed-out eye to let kids see a list or
  keep it from them, Bluetooth to pair, sliders to set up a controller's
  buttons, a key for the PIN, a lock, the way out; the hint says its word
  ("Allow", "Remove", "Look again"). A trailing "…" means it asks
  something first (a folder, a website's address, a new PIN);
- a note (the version, today's minutes) does nothing, and the hint says
  nothing.

A chevron on a row is never an action's or a choice's: building the rows
checks that a chevron row opens a page and nothing else does (a debug
assertion the flows run through; `every_settings_row_says_what_pressing_it_does`
goes through each kind). The chevrons in the margins, over and under a
list, a menu or the tabs, only say there's more that way.

### Controllers

Chanoma reads controllers' event devices itself (`crates/aizome-gamepad`,
which the login screen shares, listening without grabbing), on a thread of
its own, rather than through a library, because it **grabs** them (`EVIOCGRAB`):
while a service's page is open, Chrome's Gamepad API would otherwise also
act on presses Chanoma turns into keys. Devices are found with inotify on
`/dev/input`, retried when logind's ACL arrives (`IN_ATTRIB`), and told
apart from keyboards, touchpads and motion sensors by their capabilities.
A controller that comes back (by its Bluetooth address or USB serial)
keeps its player number.

Buttons are named by position (`South` is the bottom face button). The
mapping is, in order: one set up in Chanoma's settings for that model
(`controllers.json`); SDL's community database (the package points
`AIZOME_GAMECONTROLLERDB` at nixpkgs' `sdl_gamecontrollerdb`), numbered
as SDL numbers a device's buttons, axes and hats; else the kernel's
gamepad layout, with Microsoft's and `xpad`'s left and top buttons swapped
back (they report X, the left one, as `BTN_X`, the same code as
`BTN_NORTH`). Microsoft's Bluetooth controllers, whose right stick is
Z/RZ and triggers brake and gas, map right without the database. A pad
the database doesn't know that reports joystick codes (`BTN_TRIGGER` and
on, as cheap USB pads do) gets its buttons in order, as SDL guesses (yes,
no, left, top, the shoulders, the triggers, Select, Start); one without
a direction pad of its own has its X and Y axes as the arrows too, which
is where such pads put theirs.

Which button says yes follows the controller (Nintendo's right-hand A,
others' bottom button) unless a grown-up sets it. Menus get a direction
from the arrows or the left stick (with hysteresis), repeating after
420 ms every 130 ms, quickening to 75 ms. Select and Start held for a
second is Home on controllers without one. The pointer hides while a
controller is in hand (until the mouse moves), as on a console, and the TV
account's Chanoma starts without one.

Games read the controllers' state from shared atomics the thread keeps
current, not through messages.

### The way home

Wherever Chanoma is, and whatever is in hand, there's a way back to Home:

| | Controller | Keyboard | Mouse |
|---|---|---|---|
| Chanoma's screens | Back, Home | Esc, Home or F1 | the hints along the foot are buttons (*Back*, *Home*), as are a screen's own (*Cancel*, *Back home*) |
| A menu or the PIN pad | Back (on the pad, once its presses are taken back) | Esc | a click beside it |
| A game starting | Back, Home: called off | Esc, Home or F1 | the *Back* hint |
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
  system and save folders, rumble, stopped when a game pauses or ends (a
  controller told to rumble does until told to stop, and a core that
  isn't running says nothing more), input bitmasks, geometry and timing
  changes, OpenGL (below), and a log: a C function (`retro/log.c`, as
  it takes printf's arguments, which Rust can't) formats each line for
  Chanoma's own, a core's errors as warnings and the rest quieter. Some
  cores log without asking whether they may (Flycast, as it loads a
  game). Vulkan is declined.
- **OpenGL** (`retro/gl.rs`), for the Dreamcast's core (Flycast) and the
  PlayStation 2's (PCSX2, Play!): the context the core asks for (OpenGL,
  its core profile, or OpenGL ES 3), made through EGL as it asks
  (`libEGL.so.1`, libglvnd's, opened at run time) on the compositor's
  GPU (the device its linux-dmabuf feedback names), with no window. The
  core draws in a framebuffer of Chanoma's, as large as its largest
  picture, with the depth and stencil it asked for; `context_reset` comes
  once that's there, after the game loads, and `context_destroy` before
  it's unloaded, then the context goes. Each frame is copied on the GPU
  (`glBlitFramebuffer`, the right way up: GL's first line is the
  picture's last) into one of three textures exported as dmabufs
  (`EGL_MESA_image_dma_buf_export`), said to be XBGR8888 so the core's
  alpha isn't taken for the picture's, which the video sink shows as they
  are: nothing is read back. Where the compositor doesn't take them (no
  linux-dmabuf, another GPU, Mesa's software renderer) the frame is read
  back and goes as a software core's do, and the journal says so. The copy
  puts back the GL state it changes (the framebuffers bound, the scissor,
  rasterizer discard, sRGB), so the core's next frame finds its own; the
  picture of where a game was left is read from the last frame's texture.
  The core is asked to prefer OpenGL's core profile, and told its context
  is its own (`SET_HW_SHARED_CONTEXT`).
- **The callbacks** go in libretro's order: the environment before
  `retro_init`, the rest after it, before the game loads. Given before,
  Mesen (the NES's core) reached for its renderer, which `retro_init`
  makes, and crashed Chanoma with it: every NES game left a black screen
  (found in `docs/chanoma-user-review.md`). The tests' core loses a
  callback given before `retro_init`, as Mesen does.
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
- **A BIOS kept by the games** (`retro/bios.rs`): a `bios` folder in a
  game's folder or one above it, up to beside the games folder it's in
  (`roms/../bios`, as Batocera and RetroBat keep theirs). Of its files
  only those Chanoma's cores look for are linked into the system folder,
  each where its core looks and whatever the case of its name: the
  Saturn's (`sega_101.bin`, `mpr-17933.bin`, `saturn_bios.bin`), the
  Dreamcast's (`dc_boot.bin`, `dc_flash.bin`, under `dc`), the
  PlayStation's (`scph5501.bin` and the like) and the Game Boy Advance's;
  and one PlayStation 2 BIOS (4 MiB, from `pcsx2/bios`, `ps2` or the
  folder itself), North America's models first, under `pcsx2/bios`. The
  folders they go in are Chanoma's, so no core writes on the drive's.
  A file of the grown-up's own in the system folder stays, and links to
  a drive taken out go.
- **Which core**: a system's cores are tried best first; one that plays
  nothing without its console's BIOS (Beetle Saturn, PCSX2) is chosen only
  where that's there, else the next (Yabause and Play!, which stand in
  for it). PCSX2's BIOS counts only as a 4 MiB image in `pcsx2/bios`, not
  the settings PCSX2 writes beside it, which stay when the drive with the
  BIOS is taken out. Flycast stands in for the Dreamcast's BIOS by itself
  where it isn't there.
- **Only free BIOS images** (`CHANOMA_FREE_BIOS=1`, for checking that
  every system plays as on a new installation): no `bios` folder by the
  games is looked in, and cores get a system folder of their own,
  `system-free` beside `system`, holding only `$CHANOMA_BIOS`'s (OpenBIOS),
  as `system` keeps links to a console's own BIOS from earlier games. So
  the PlayStation plays with OpenBIOS, the Saturn with Yabause, the
  PlayStation 2 with Play!, and the Dreamcast and Game Boy Advance with
  their cores' stand-ins. Flycast's memory cards and settings are in the
  system folder, so the Dreamcast's saves are kept apart in this mode. Flycast draws at twice the console's 640 by 480
  (`reicast_internal_resolution`: its options keep Reicast's names), for
  a television.
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
- **Starting**: Back and Home call a game off while it starts, at once,
  however long its core takes. After 15 seconds a menu says it's *Taking
  a long time to start* (or that *The game before is stuck*) and asks:
  *Stop it now* (first: a start that long is more likely stuck than
  slow) or *Keep waiting*, with Back and Home still stopping it after, as
  the screen then says. A game goes into *Keep playing* once
  it has started, and leaves it when it fails to start or is stopped
  after that long, so that a kid doesn't come back to it first. A core
  given up on is told to end and isn't waited for, as it may never come
  back from loading; until it does, the next game waits for it (two of one
  core in a process don't mix) and is asked about the same way.

### Services

Each service opens in the browser the system gave Chanoma
(`CHANOMA_BROWSER`, Google Chrome by default) or the grown-ups' choice,
else the first of `google-chrome-stable`, `google-chrome`, `chromium` and
`chromium-browser` on the PATH, with `--kiosk`, a profile of its own under
`$XDG_DATA_HOME/aizome/chanoma/browser/`, no first-run or crash-restore
bubbles, no translation or casting UI, and `--password-store=basic`.
YouTube gets a television's user agent, which serves its TV interface.

The services that protect their shows with Widevine (`Service::drm`:
every paid one, and Plex's and Kanopy's free films; not PBS KIDS or
YouTube) play only in Google Chrome, which has the decoder; Chromium
hasn't. In another browser such a service doesn't open: the screen says
it needs Google Chrome, where its own page would show an error code.

**Sign-ins are kept in the profiles, without a keyring.** Chrome encrypts
cookies and saved passwords with a key it keeps in the desktop's keyring
when it has one; `--password-store=basic` has it use its built-in key
instead, so a service stays signed in from one evening to the next with
no keyring daemon running, nothing to unlock and no prompt on the
television (checked in a real browser with no session bus, below). The
profiles are as private as the account's home (0700), and as the disk's
encryption makes them at rest. A keyring would add nothing for the TV
account: it signs in with no password, or a PIN, so there's nothing to
unlock a keyring with, and it would either ask on the television or keep
its contents unencrypted. The choice stays fixed: sign-ins written under a
keyring's key can't be read without that keyring.

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

**Which ones kids see** (*Films and shows* in the settings): new ones
show at once unless set to wait for a grown-up (`new_films`); each film
and show is a switch, and all of them are, at once. Set to wait, what
kids could watch stays theirs (`films_allowed`). Kids' Home, *Everything*
and *Keep playing* hold only theirs. A film or show is known by its
folder, or its file (`Shelf::key`).

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
console's passcode is (`crates/aizome-sofa/src/pinpad.rs`, shared with the
login screen, which takes the TV account's PIN on it). Each press is the
same dot in its cell, and nothing in the legend lights, so someone
watching sees that a button went but not which. The legend is ten
buttons, one for each digit, in the same place on every controller: the
arrows are 1 to 4, the shoulders 5 and 6, the top and left face buttons 7
and 8, and the triggers 9 and 0, so the legend reads 1 to 9 and then 0, as
a keyboard's row of numbers does. They're named by place, as every button
is here, so a PIN made on one controller is the same presses on any other,
whoever made it; the legend draws each in the prints of the controller
pressed last, with a mark under the face buttons saying which place they
are (Xbox's Y is Nintendo's X). Left out: the yes and no buttons, which
Nintendo's controllers swap (they're Done and Delete), and the sticks. The
arrows count as presses of the direction pad only, once each, never the
stick or a press held. A trigger counts once pressed past half way. Delete
(the no button, Backspace, or the pad's Delete clicked) takes the last
press back, and with none, puts the pad away. Each of the legend's
buttons has its digit in its corner: a PIN is kept as those digits (1 is
Up), so PINs made before the triggers joined are the same presses. The
legend never changes as presses go in, so the numbers give nothing away.
A keyboard presses them with the digits, or with the arrows, Page Up and
Page Down, Tab and Space, the keys Chanoma's hints show for those
buttons; the legend shows those keys when a keyboard was last used.

A controller without a digit's own button presses it as a chord, Select
being shift (Start on a pad without Select): Select with a shoulder is the
trigger behind it (a Super pad's 9 and 0), Select with an arrow is the
digit four on (5 to 8 on a NES pad), and the face buttons stand in where
nothing else is left (a NES pad's 9 and 0). Only a missing button gets a
chord, chosen so that no two digits share one, and the legend draws the
chord in its digit's place ("− + L" on an 8BitDo SN30). Which buttons a
controller has comes from its layout (`PadInfo::buttons`). Every mix of
missing shoulders, top buttons and triggers is checked to press each
digit one way and no two alike.

It's kept as PBKDF2-HMAC-SHA-256 with a random salt and 150,000
rounds (about a tenth of a second, checked off the interface thread).
Three wrong PINs are free; then the pad waits 30 seconds, doubling each
time up to 15 minutes, and the count is kept in `lockout.json`. This keeps
out kids with a controller; it can't resist someone who can run code as
the TV's user (who can read the hash, or `aizome-chanoma --reset-pin`).

**Forgot it?** (Start on the pad, when the PIN is asked for) signs in
with the password of the computer's account Chanoma runs as
(`app/reset.rs`), typed on Chanoma's keyboard (dots in its box) or a
real one, and checked through PAM: `aizome_sys::pam`, the lock screen's
service `aizome`, moved from the shell to `aizome-sys` (its `pam`
feature) to be shared. Right, it's grown-up mode and a new PIN to
choose; wrong, it counts with wrong PINs. It's that account because PAM
without privileges checks no other's password. The TV account's Chanoma
(`--tv`) doesn't: its password may be nothing, or the PIN a kid logs in
with, so there *Forgot it?* says a grown-up can have Chanoma forget the PIN
from their own account (Settings, Users & Groups, the TV account's *Forget
Chanoma's PIN*, which runs `aizome-chanoma --reset-pin` as the TV account
through the TV account's helper).

Time counts while a game, a service or a film is open in kids' mode,
into `usage.json` (written every 30 seconds while it counts). The gate
is open with the least of today's limit (plus grants) and the time until
bedtime left. A grant tops today up from what's been used, so a grant
after time ran out is all play, and lifts bedtime that long.

### Folders

Chanoma looks through one list of folders for everything it shows,
games, films and shows alike, each to any depth (`Settings::folders`;
settings written before kept games and videos folders apart, and are
merged as they're read). *Settings › Folders* lists them, each with
where it is ("On Extreme Pro", "In your home folder", said the same
while its drive is out) and what it holds ("6509 games for 31 systems ·
2 films"), how far a look through it has got ("Looking through it: 12 of
its 31 folders, 4,812 files", from the files index's progress), or that
its drive isn't plugged in, is being opened, or couldn't be opened.

**A look that takes a while shows on Home too**, quietly, in the header
before the controllers and the clock. Every start looks, for games added
or taken away since, reading again only the folders that changed, so it
says so: "Checking Extreme Pro for new games", how far
through its own folders it is ("12 of 31 folders", and a bar), and how
many files so far, the number whose climbing says how fast it goes. It
never guesses how long is left. How far is known because the index reads
a folder depth first: each of its folders is read whole before the next,
so the count of them done only grows (`library::Look`, from the index's
`Report`, which counts files as well as folders). A folder without
folders of its own shows the count alone. After the reading, "Recognising
the games" until they show. Then what it found stays for eight seconds
("Checked Extreme Pro · Nothing new", "3 new games", "1 game gone"), so a
look that ends while a toast covers it ("Grown-up mode" sits over the
header's middle) or a screen with a title of its own hides it still says
it ended. A look the index answers at once (a folder
read before and unchanged: its only word is that it's over) shows
nothing, so starting Chanoma doesn't flash it. A folder opens
a page to look again or remove it (which asks first, *Keep it* ready to
press, as removing a website does; nothing on the disk is deleted).
Games and Films and shows have *Your folders* and *Add a folder…* too.

**Drives are opened as Chanoma starts and as they're plugged in**
(`app/drives.rs`, over `aizome_sys::disks::Automount`): a television's
films and games are often on a USB disk, and nobody opens it in Hako
first. Each filesystem UDisks says to mount by itself (`HintAuto`:
removable and external drives, never the system's own) is mounted the
first time Chanoma sees it unmounted, off the interface thread, and the
library is read again once it is. One already mounted, or unmounted later
in Hako, is left as it is; one unplugged and plugged in again is new.
Locked (encrypted) drives wait for someone with the passphrase. A drive
that can't be opened is said once, in a note at the top ("Couldn't open
the drive Extreme Pro"): not again while it stays broken, nor at the next
start (`drives.json` in Chanoma's state remembers), but again if it opens
one day and later fails. *Settings › Folders* keeps listing the drives
that couldn't be opened, with why, and tries one again when it's
pressed. `--no-drives` leaves drives alone (a preview beside a desktop).
Each drive is mounted once: a filesystem that drops out of UDisks's view
for a moment (as it probes a drive, or mounts it) isn't mounted again, and
one mounted meanwhile by someone else counts as opened where it is
(`Disks::open`). A drive another account has open is under that account's
`/run/media/USER`, which this one can't read (a grown-up's desktop opened
it, then the TV account logged in): it's said the same way, once, with
what to do (eject it there, or unplug it and plug it back in). The TV
account's drives are the exception: every account may read them (under
The TV account).

*Add a folder…*, and Home's *Add games*, open a picker of Chanoma's own
(`folders.rs`, `app/picker.rs`, `ui/picker.rs`), as the kit's file panel
wants a pointer and a keyboard. It's laid out as the settings are:

- **Suggested** comes first: the folders that look like a collection,
  each added with one press (right looks inside it first). Home's
  folders, and each drive and the folders at its top, are listed once
  each. A folder with folders named for systems (`snes`, `Super
  Nintendo`), or three games of its own, or named for games (`roms`,
  `Games`) with either, is games; one with three videos, or named for
  them (`Movies`, `TV`) with videos in it or in its folders, is films and
  shows. A folder named for games with neither is looked into a level
  deeper (`Emulation/roms`, Batocera's `share/roms`). That's two or
  three levels, never a whole drive, so a cold USB drive answers in a
  moment where reading all of it takes minutes. The most systems come
  first; folders already looked through aren't offered. With nothing to
  offer, and nothing pressed yet, it goes to home.
- **Places** down the left: home, the usual folders in it that exist
  (`Games`, `ROMs`, `Videos`, `Movies`, `Downloads`), each drive under
  `/run/media/$USER`, `/media/$USER`, `/media` and `/mnt`, and the whole
  computer. An empty folder there is a drive's mount point while it's
  out, and is left out. The places are read again every second while the
  picker shows, so a stick plugged in appears, and *Suggested* is looked
  for again. A drive pulled out from under the folder showing goes back
  home.
- **The list** on the right: where the folder is, in words ("Home ›
  ROMs › snes"); then the choice, *Add this folder*, with what's directly
  in it ("Folders for 31 systems: Game Boy, Genesis, NES…") or what the
  files index knows is in it as of its last look, or else that it's one
  of the folders already, or inside one; then the folder's own folders,
  hidden ones left out, in natural order, each with a word on what's
  directly in it ("12 games · 3 folders"), *Added* on those looked
  through already.
- **Browsing reads no deeper than a folder's folders**: one listing for
  the folder showing, and one for each of its folders (not in a folder of
  more than 400). A folder is looked through to the bottom once it's
  added, in the background, and nothing goes into the files index for a
  folder passed by. Before, the choice counted what Chanoma would find,
  which read all of a drive just to show its top.
- **Adding** a folder inside one already is refused, with a toast saying
  which holds it. One holding some already is added beside them, and a
  file in both is the nearer folder's (`library::read_all` reads the
  deepest first), so its games keep their keys, and with them what kids
  may play and where they were left.
- **Off the interface thread:** folders are read there, one reading per
  folder shown. Each answer carries its reading's number, and an older
  one is dropped.

The controller:

| Button | In the list | Among the suggestions | Among the places |
|---|---|---|---|
| Up, down | move | move | move (the list follows) |
| Confirm | open the folder (the first row: add this one) | add it | into the list |
| Right | open the folder | look inside it | into the list |
| Back | up a folder, the focus on the one left (from `/`, to the places) | to the places | put the picker away |
| Left | to the places | to the places | |
| Top face button (Y, △; Tab) or Start | add the folder showing | add it | add what's showing |
| Shoulders | a page up or down | a page up or down | |

Adding goes back where the picker was opened, the focus on *Add a
folder…*, so pressing again adds another; from Home's, to Home.

What's found in a folder, whatever was added:

- **Games** by their system's folder (`snes`, `Sega - Saturn`, `ps2`) or
  their extension. A cue sheet or playlist known only by its extension
  (not in a system's folder) is a game's only if it names a disc image
  (`.bin`, `.chd`…), not an album's music or a playlist of videos.
- **Discs** that several systems' come as (a cue sheet, a CHD, an ISO, a
  CSO), outside a system's folder, by what's on them (`disc.rs`): a
  Saturn's data track starts `SEGA SEGASATURN `, a Dreamcast's
  `SEGA SEGAKATANA `; a PlayStation's `SYSTEM.CNF` boots with `BOOT`, a
  PlayStation 2's with `BOOT2`. A cue sheet's data tracks are read in
  order (a Dreamcast's in Redump's form has its system area in the
  high-density area's), a CloneCD image's `.img`, an ISO's sectors whole
  or user data alone; a CHD's track list says a GD-ROM (a Dreamcast's)
  before anything is decompressed, and a CD's or a DVD's first sectors are
  read through the `chd` crate, as a CSO's are through its blocks. A GDI
  or CDI is a Dreamcast's, a PBP a PlayStation's, by name. Anything else
  is no game of Chanoma's: a Mega-CD's, a PC Engine CD's, a PSP's, a
  computer's installer. What a disc is is read once, on the refreshing
  pass, and kept in the files index beside it (`chanoma.disc`, with its
  serial); a start's first look takes what was kept. Before, every cue
  sheet and CHD outside a system's folder was the PlayStation's, a
  Saturn's or a Mega-CD's included.
- **Films and shows**, but not videos in a system's folder
  (`roms/snes/videos`, as ES-DE scrapes them): those are games' clips. A
  video's shelf is the folder it's in, past seasons, discs and extras
  (`Season 1`, `S02`, `Disc 2`, `Extras`): several videos make a show,
  one a film in a folder of its own. A video straight in the folder
  added, or in one named for films and shows (`Movies`, `TV`, `Kids`), is
  a film of its own.
- **Each folder is read once** for both, from the files index
  (`library::read_all`), counted per folder for the settings
  (`library::Held`).

### A big collection

A household's games can be thousands of files over a dozen systems, on a
drive beside its backups. Games' settings handle that a system at a time
(`collection.rs`, `app/grownups.rs`):

- **Read through the files index** (`aizome-index`,
  `docs/files-index.md`): the folders are read to any depth,
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
- **Search every drive for games** (in Folders) looks through home and
  each drive plugged in (the picker's places) and lists the folders of
  games it finds, by place and system, each with how many and where ("2
  games in roms › snes"). It shows what the index knew first, then each
  place as it's read, with how many folders so far. A system's games
  count at the nearest folder named for it (`roms/psx`, where each disc
  has a folder of its own); others at their own folder if it holds three
  or more, so a stray `.bin` in a backup isn't a collection. Each is a
  switch: on adds it as a folder, off takes it away. Files are counted
  by name, so a disc both in a playlist and on its own counts twice here
  (not on Home).
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
BY-SA 4.0, credited in the settings' Chanoma section), 20 files, 27 MB,
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
  most); for a disc, its serial, read as its system is (`disc.rs`, above,
  and kept with it): the one a PlayStation's or a PlayStation 2's
  `SYSTEM.CNF` boots (`BOOT = cdrom:\SCUS_949.00;1` is `SCUS-94900`), or
  a Saturn's or a Dreamcast's product number from its system area
  (`MK-81009`). Serials are compared as letters and digits, and without
  Sega's `MK-`, which the database has for some games and not others
  (`MK-51035` is the entry's `51035`). A Dreamcast CHD's system area
  isn't read (its track list says it's a GD-ROM), and PBP, CDI and ZSO
  discs aren't read.
- **Else by its name with the tags a database adds**: a file named as
  No-Intro and Redump name them (its region in brackets) matches a name
  that only adds more after it, as a database adds a release's languages:
  the Dreamcast's `Crazy Taxi (USA)` is the database's `Crazy Taxi (USA)
  (En,Ja)`. Of several, the one with fewest; a name without a tag isn't
  guessed at.
- A game named twice in a database (a release per disc or revision) takes
  what any of its entries says.
- **Read once a start, all at once**: the systems' databases are read on
  a thread each, once for both looks at the folders (`Catalogs`), and the
  facts a folder at a time (`Index::facts_in`). Looking each file up on
  its own was most of a start with thousands of games.

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

### Pictures

A game's tile shows the first of these there is:

- **Its box beside it**: what its folder's `gamelist.xml` names
  (EmulationStation's and Batocera's: its `thumbnail`, else its `image`,
  else its `marquee`); else a picture named as it is (`Game.png`), or as
  Batocera's scraper names them (`Game-thumb.png`, `-boxart`, `-box`,
  `-cover`, `-image`), in its folder or a pictures folder beside it
  (`media/`, `boxart/`, `images/`, `covers/`, libretro-thumbnails'
  `Named_Boxarts/`). Only pictures the files index already has are
  looked for.
- **Its box from libretro-thumbnails** (`art.rs`), as RetroArch fetches
  it: by the name its database knows it by (`Game::known_as`, found by
  its name, CRC or disc serial, so a renamed game's too), from
  `thumbnails.libretro.com/<database>/Named_Boxarts/<name>.png`, else
  (none there) by its file's name, as the thumbnails sometimes name a
  game where the database adds its languages (the Dreamcast's `Crazy
  Taxi (USA)`, `Crazy Taxi (USA) (En,Ja)` in the database), as its
  tile comes near the screen, four at a time, and kept in
  `$XDG_CACHE_HOME/aizome/chanoma/art/<system>/`. One that isn't there
  leaves a `.none` beside where it would be, and isn't asked for again
  for a week; with the server out of reach, none is asked for two
  minutes. *Box art from the internet* in Games turns this off.
- **Where it was left**, for a game played before.
- **Its sprite**: eight by eight squares, mirrored as an invader is, in
  its system's colour and a lighter one, made from its title, so a
  game's releases share it and a kid can know it again without reading.
  The system's name beside it gives way to its initials (GBA) where the
  name and the corner's *New* don't both fit.

### Search

Y (the top face button, Tab) on Home or among the games opens search
(`app/search.rs`), as does typing on a keyboard there: the games and the
films and shows whoever's watching may open (kids, theirs alone), by
name. Each word typed must start a word of the name or of its system's
names (`snes mario`), or, three letters and more, be inside the name;
accents go as typed (`pokemon` finds Pokémon). The name starting so comes
first. Results change with each letter, above Chanoma's keyboard (on the
controller: X takes a letter back, Y types a space); Start or Back puts
the keyboard away to go through them, up from their first line brings
it back, and Back from them goes back where search was opened.

### The TV account

A grown-up can give the television an account of its own (Settings,
Users & Groups, *TV account*): `tv`, "TV", signing in with nothing, a PIN
or a password, as they choose. At the login screen it's the last tile, a
television; with nothing to sign in with, choosing it shows Start (it
never starts by itself, or logging out would start it again), with a PIN
the PIN pad takes it from a controller (the same buttons as Chanoma's
grown-ups' PIN, 1 to 9 and 0), and with a password the login screen's
keyboard does. Its session is Chanoma's, whatever's picked: the login
screen doesn't offer another, Aizome's session hands over to it, and
logging in by itself (Settings, *Log in automatically as*) starts it too.

The session (`data/aizome-chanoma-session`) is the compositor in kiosk
mode (`aizome --kiosk -- aizome-chanoma --tv`): no shell, none of the
desktop's shortcuts, nothing of `aizome-session.target`, the system's
environment file (Chanoma's cores, BIOS, browser and databases), and the
compositor stops when Chanoma does, ending the session. What it says goes
to the journal as `chanoma-session`. Chanoma with `--tv` is Chanoma as
anywhere else (kids' mode from the start; a first start asks a grown-up
for the PIN and the setup; the PIN unlocks grown-up mode), but leaving is
*Log Out*, which kids may choose too (beyond is only the login screen),
and *Forgot it?* doesn't sign in with the account's own password.

The account's own systemd starts only what Chanoma and its browser need:
the session bus, PipeWire and its PulseAudio side (WirePlumber with
them), speech for pages that ask, and the account's NixOS activation and
temporary files (`programs.aizome.chanoma.tvUnits`). Everything else a
login starts for every account (an SSH agent, a location agent, rootless
Docker, whatever a desktop adds) is masked for it alone, by a generator
its systemd runs as it starts (`aizome-tv-units`): it reads the account's
name from the record, and masks each unit `default.target`,
`basic.target`, `sockets.target`, `timers.target` and `paths.target`
pull in but those kept, in the generator's early directory, which comes
before `/etc`. Services D-Bus starts when something asks (the portals)
are as they were. No keyring runs: nothing in the session asks for one
(the browser's sign-ins, under Services).

The drives the account opens every other account may read, but not
change (`programs.aizome.chanoma.shareTvDrives`, on unless turned off).
UDisks mounts them in `/run/media/tv`, and makes that folder for its
account alone unless it's there already: the module makes it at boot
(systemd-tmpfiles) for every account to look in. On a FAT, exFAT or
NTFS drive, which has no owners of its own, every file is the TV
account's, so others may read and not write; a Linux filesystem's own
owners and permissions apply as they would anywhere. A drive stays open
after the account logs out, so a grown-up's account can read it then
without unplugging it. Chanoma keeps its folders by their paths, though:
one a grown-up's Chanoma added while the drive was open in their own
account (`/run/media/USER/…`) isn't found while it's open in the TV
account's.

The account is made and changed by `aizome-tv-account`, Settings' helper,
run as root through pkexec under the polkit action
`org.aizome.Settei.tv-account` (an administrator's password): `useradd`
with nologin for a shell (so a console, `su` or SSH won't let it in;
greetd starts its session itself), the password set as a hash with
`usermod` or emptied for none (greetd's PAM service is `login`'s, which
has `nullok`), and its record `/var/lib/aizome/tv-account.toml` (the
account, how it signs in, a PIN's length; nothing secret), which the
login screen, Aizome's session and Settings read (`aizome_sys::tv`).

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
  and extension, cue sheets' tracks, box art, shows), discs told apart by
  what's on them (Saturn, Dreamcast, PlayStation and PlayStation 2 discs
  as cue sheets, CloneCD images, ISOs, GDIs, CSOs and CHDs of CDs, DVDs
  and GD-ROMs the tests write; a Mega-CD's, a PSP's and a computer's
  none of them; read once, then known from the index), BIOS images by a
  collection's games offered as the cores look (the PlayStation 2's,
  North America's first; a grown-up's own kept; a drive's taken out let
  go, PCSX2's settings left behind not taken for its BIOS) and the core
  chosen by them, none of them with only free BIOS images, through the files
  index, layouts, and a language chosen found again in the next episode
  (by name, `en` as `eng`; by place without one; none rather than
  another language), and the sites a page may be on (subdomains; not
  look-alikes, a user before the host, the rest of Google, or what isn't
  the web), which services get Chanoma's keyboard, the PIN's buttons
  (ten, the triggers 9 and 0; none of yes, no, Start or Select), the chords
  of every controller short of them, and where clicks on its pad land, and the keyboard's rows, moving between its keys, Shift and its
  symbols. What plays, where a film carries on and the subtitles' cues
  are `aizome-media`'s, tested there; serving the media controls is
  `aizome-mpris`'s, tested there against a private `dbus-daemon` (what a
  player says, and which asks reach it).
- **Headless** (`tests/chanoma.rs`, also in the package's check):
  starting (the name from the first frame, buttons doing nothing, until a
  games' database the test holds back as a pipe is read); the first
  setup, its PIN pressed on the controller (a slip taken back with B;
  the stick, Start and Select pressing nothing; the right trigger a 0)
  and again on the keyboard; Home for kids and grown-ups (the PIN typed by
  its numbers on a keyboard, 9 and 0 among them); a PIN made on an Xbox
  controller pressed on a Nintendo one, its B taking a press back; a pad
  without triggers pressing 9 and 0 with Select and a shoulder, the legend
  saying so; the TV account's Chanoma (kids may *Log Out*, which ends it,
  and *Forgot it?* sends a grown-up to their own account); wrong
  PINs and their wait, surviving a restart; a game played through a
  libretro core written in the test (frames reaching the window's video
  sink enlarged, the controller reaching the core, pausing under the Home
  menu, its state and picture kept, *Keep playing*, carrying on); games
  that core keeps loading until the test lets go (Back calling one off
  at once, the next game waiting for that core and asked about after 15
  seconds, kept waiting for and starting when it lets go; *Stop it now*
  taking it out of *Keep playing*; the question put away with Back, and
  Home stopping it after; a slow one starting under the question, which
  goes); a service that protects its shows saying it needs Google Chrome
  in a stand-in named `chromium` and opening in one named
  `google-chrome-stable`, with PBS KIDS opening in Chromium; every
  system's tab, one game for each of the systems Chanoma plays, *All
  games* leading the shelf, RB along them all with the chosen tab whole
  on the screen and named beside the title, LB round to the last, and a
  tab clicked; a folder being checked shown on Home (a look the index
  answers at once not shown; "Checking Games for new games", 12 of 31
  folders, 4,812 files and the bar; then recognising the games; then
  "Checked Games · Nothing new" for eight seconds, and "1 new game" after
  another look); a game's rumble (the test core rumbles while B is held)
  stopped as it pauses and as it ends, and started again as it carries on; time running
  out mid-game, with the warning, the game kept, the closed
  screen, and a grown-up's grant; bedtime; a folder added with the
  controller alone (*Suggested* offering a drive's `roms` and home's
  `ROMs`, the drive's first; one press adding it, the settings back on
  *Add a folder…*, then its games and its film read but not its game's
  clip, and its row saying so; offered no more; the places, a folder's
  folders with what's directly in each, up again to the folder left, a
  drive among the places with its folder marked *Added*, that folder and
  one inside it not added again, and the picker put away unchanged; its
  game's box named by the folder's `gamelist.xml`; its page's *Remove
  roms* asking first, *Keep it* keeping it and *Remove it from Chanoma*
  removing it); the way
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
  controls' play and stop). A big collection: a No-Intro-named set in a
  folder and a drive plugged in beside a backup, one of each game
  shown (no beta, no BIOS), the Systems page turning the Game Boy off with
  one switch, every region and then the beta shown and hidden again, a
  system's games all let in for kids and all kept out, the shoulders
  jumping a letter, and *Search every drive for games*, reached from
  Games' *Your folders*, listing the drive's
  SNES folder where it's named (not the backup's stray `.bin`) and adding
  it, its games joining one of each. What the databases know: a zipped
  game and a renamed one (found by its CRC) beside one found by name,
  each saying its year, genre and players; a second controller bringing
  *For 2 players* before Games with the focus kept on Games, the zipped
  game playing through the stand-in core from its unpacked file, and the
  shelf going with the controller. A forgotten PIN chosen again with the
  account's password (a stand-in for PAM): *Forgot it?* on Start, the
  account named, a wrong password said and counted and not kept, the
  right one typed on a keyboard with a slip taken back with X, then a
  new PIN. Films and shows kept from kids one by one: a film switched
  off, new ones set to wait, a film added waiting with *New*, and kids'
  Home showing the show alone. Search: a kid typing at Home finding only
  a film of theirs (not a game kept from them), Back and Back again; a
  grown-up with the controller, Y opening it, A typing the keyboard's
  first key, X taking it back, a name's words and its system's, Start
  putting the keyboard away and up bringing it back. The test's core
  loses a callback given before `retro_init`, as Mesen did, so the order
  is checked by every game played. The unit tests add where clicks land:
  the hints, the bar's *Home* in a 16:10 window, beside a menu; the
  names' tags (regions, revisions, variants, a BIOS, discs and modes kept
  apart, dates) and the region a language puts first; Mac resource forks
  not taken for games. Zips are read stored and deflated, checked by
  their CRC (a damaged one refused), unpacked, and the oldest cleared; a
  database is read and games found in it by name, CRC and serial (Sega's
  with its `MK-` or without, either way round), and by a name its
  database adds tags to (the fewest; never another region); a disc
  image (written by the test) says its serial, from a cue sheet and a
  playlist. Folders say what's directly in them, and *Suggested* finds a
  drive's `roms`, home's `Emulation/roms` and `Movies` (not a backup's
  games, deeper); box art's names and addresses are libretro-thumbnails',
  and one is fetched from a server of the test's own (found in the second
  of two databases), a miss marked and an unreachable server not;
  one missing by its database's name is asked for by its file's, and one
  the databases don't know isn't asked for;
  `gamelist.xml` says each game's picture, its box first; search ranks
  a name's start, its words' starts and the system's names, accents as
  typed; the PIN pad's foot holds *Forgot it?*.
- **OpenGL** (`tests/gl.rs`, opt-in, with a GPU through EGL): a core the
  test writes asks for OpenGL 3.3's core profile as its game loads, and
  draws red over blue in the framebuffer it's given, leaving a scissor
  on. Through a whole session: `context_reset` once the game's loaded, the
  frames copied the right way up (the picture where it was left is red
  at the top), its scissor still on at its next frame, and
  `context_destroy` before `retro_unload_game`. On 2026-11-08 on
  wx-workstation's `/dev/dri/renderD128`, its frames read back (no
  compositor): passed.
- **Real cores** (`tests/cores.rs`, opt-in): every system's cores from
  nixpkgs load and list the extensions Chanoma sends them; gambatte,
  SameBoy and mGBA play a Game Boy game the test writes, whose screen
  turns black while a direction is held, through whole sessions with the
  controller state, and carry on from their saved state. On 2026-10-31
  with nixpkgs' cores of that date: all three passed (mean brightness
  249–252 untouched, 3 with Right held). On 2026-11-08 the Saturn's,
  Dreamcast's and PlayStation 2's (Beetle Saturn, Yabause, Flycast,
  PCSX2, Play!) loaded and took every extension Chanoma sends them, but
  for Play! and a playlist (`.m3u`), which goes to PCSX2. Each core is
  also given a file that isn't a game, as a game of its system's,
  through a whole session (a cue sheet or GDI naming a track that isn't
  there, else bytes that are no disc): it must say it can't, or start on
  it, and not take Chanoma down, as Flycast did on ganymede on
  2026-11-08 by logging through a log it hadn't been given. On
  2026-11-09 all six said they couldn't, Play! (built with the system's
  C++ runtime) once it had started: "Play! stopped: No 'SYSTEM.CNF' file
  found on the cdrom0 device", caught by `retro/guard.cpp`. Before the
  log, Flycast alone took the test down (SIGSEGV); before the guard,
  Play! did (an exception through Rust's frames), and with its own C++
  runtime, alone, it broke Mesa's LLVM starting up (`pthread_once`
  calling 0). Yabause, given a cue sheet of random bytes, crashes in its
  own `YabSetError`: hence sheets that read.
- **Dreamcast games** (`tests/cores.rs`'s `dreamcast_games_play_and_carry_on`,
  opt-in, with `CHANOMA_TEST_DC_GAMES`, `_ROOT`, `_OUT` and `_STAGES`):
  as the PlayStation's below, through Flycast with Chanoma's settings
  (1280 × 960), drawing with OpenGL on wx-workstation's GPU (read back),
  with the BIOS a `bios` folder beside the games holds, offered as
  Chanoma offers it. On 2026-11-09, seven US and Japanese games from a
  household's own drive (CHDs, and a cue sheet's folder): Crazy Taxi (and
  2), Ikaruga, Power Stone 2, Soulcalibur, Street Fighter III: 3rd
  Strike and WWF Royal Rumble, six stages each, each carrying on from the
  last: all started from the BIOS's logo and carried on every time, in
  races, fights and menus by the last, drawn right, with nothing crashed
  through 42 starts and stops in one process.
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
  A third signs in to a service of the test's (a cookie lasting a day),
  ends the browser as Chanoma does, and starts it again on the same
  profile: the service's next page is asked for signed in. Every run has
  no session bus (`DBUS_SESSION_BUS_ADDRESS` at nothing), so no keyring.
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

- **A test TV** (2026-11-02, on the X1 Carbon: a release build on a
  headless Aizome compositor of its own, with a library of stand-ins named
  from libretro-database and freely licensed homebrew games): the NES's
  240p Test Suite starting in Mesen with the callbacks in order, where
  before every NES game crashed Chanoma; box art fetched from
  libretro-thumbnails for the stand-ins the databases know, sprites for
  the rest; search finding 13 games for `zelda`, their boxes fetched as
  they showed. Two end users' reviews of the build before are in
  `docs/chanoma-user-review.md`.

Not yet checked: a physical controller (Bluetooth pairing, and the
mappings of real pads: Xbox, PlayStation, Switch Pro, 8BitDo), the
Bluetooth pairing screen against BlueZ, DRM services (Netflix and the
like) in Google Chrome, sound, the ISO, or a television. These need doing
before calling any of it tested on hardware.

## Limits

- A core stuck loading for good keeps its thread in Chanoma's process:
  Chanoma comes back Home, and films and pages work, but no other game
  starts until Chanoma is opened again (the screen says so).
- Box art is fetched by each game's name from thumbnails.libretro.com,
  a server of the libretro project's, as its tile shows: it hears which
  games are on the TV. *Box art from the internet* in Games turns that
  off. A game the databases don't know has no name to fetch by, and shows
  its sprite.
- Search goes by names alone (not genres, years or players), and finds
  only the games that show (one of each, no variants, systems on).
- A forgotten PIN is reset with the password of the account Chanoma runs
  as, and no other: PAM without privileges checks no other account's.
  Sekka has no kids' accounts yet to keep from doing it.
- Chanoma's keyboard says the shoulders move the caret, which they do
  over a page, but not in search or signing in.
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
- No Vulkan cores; no disc swapping for multi-disc games; no touch
  screens (the DS). The Nintendo 64 and GameCube have no system here.
- OpenGL cores' frames have been read back in the tests (the tests' own
  core, and seven Dreamcast games through Flycast); sharing them with the
  compositor as dmabufs hasn't been tried in them, nor a PlayStation 2
  game (there's none free to). Play! (the PlayStation 2 without its BIOS)
  plays fewer games than PCSX2, and no playlists; a disc it can't boot
  ends its game with what it said. Chanoma doesn't help anyone come by a
  console's BIOS: it's the console maker's, and a grown-up copies it from
  their own console. Its screens don't say which core plays a game or
  whether a BIOS was found; only Play!'s failure points to PCSX2 and the
  `bios` folder.
- A core that crashes takes Chanoma with it (cores run in its process),
  and Chanoma starts again saying so (After a crash): the game since it
  last saved is lost. A core that hangs without crashing isn't noticed.
- Discs are told apart only among Chanoma's four disc systems: a disc of
  another (a Mega-CD's, a PC Engine CD's) isn't shown, even in a folder
  not named for it.
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
- *Search every drive for games* reads every folder of home and each
  drive each time it's asked; a drive just plugged in takes minutes on a
  slow USB stick or exFAT drive (`docs/files-index.md`), showing what it
  found as it goes. *Suggested* reads two or three levels instead, so it
  misses a collection deeper than that under a folder not named for
  games. Nothing looks by itself when a drive appears.
- Adding a website needs a keyboard and a mouse: its address is typed
  in a dialog of the kit's, where Chanoma's keyboard doesn't come.
  Adding folders doesn't: see Folders above.
- Subtitles lose ASS styling (italics and top placement are kept), as in
  Utsushi. A subtitle file beside a film with no language in its name is
  called *Track 1*. There's no household language to start every film
  in: a choice is kept per show or film. Choosing a language hasn't been
  tried on a film with several sound tracks; the engine's test media
  have one each, so the matching is checked in unit tests only.
