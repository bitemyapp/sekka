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
packages are allowed), exports `CHANOMA_CORES` (and `CHANOMA_BROWSER`, for
a browser given) to the session, and sets BlueZ to connect quickly and to
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
12-hour setting, read at start.

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
| A menu or the PIN pad | Back | Esc | a click beside it |
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
  plays through PCSX-ReARMed's software renderer, which needs no BIOS).
  Logging is declined too (a C varargs callback).
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

### Rules

The PIN is kept as PBKDF2-HMAC-SHA-256 with a random salt and 150,000
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
  `scan_videos`, five folders deep and at most 20,000 files), so the
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

## Checking it

```sh
cargo test -p aizome-chanoma                                   # unit and headless
CHANOMA_SHOTS=/tmp/shots cargo test -p aizome-chanoma --test chanoma   # with pictures
CHANOMA_TEST_CORES=<folder of *_libretro.so> \
  cargo test -p aizome-chanoma --test cores -- --ignored --nocapture
```

- **Unit tests** cover the PIN hash (against RFC 7914's PBKDF2 vector),
  lockout, limits, bedtime across midnight, grants, ioctl numbers,
  controller capabilities, SDL database numbering (a Microsoft Bluetooth
  controller's entry), the kernel layout for Microsoft's and Sony's
  drivers, events into buttons and axes, navigation repeats and stick
  hysteresis, the sound ring and resampler, pixel conversion and
  enlargement, core options, game and video scanning (systems by folder
  and extension, cue sheets' tracks, box art, shows), and layouts.
- **Headless** (`tests/chanoma.rs`, also in the package's check): the
  first setup by controller and keyboard; Home for kids and grown-ups;
  wrong PINs and their wait, surviving a restart; a game played through a
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
  keyboard leading home). The unit tests add where clicks land: the
  hints, the bar's *Home* in a 16:10 window, beside a menu.
- **Real cores** (`tests/cores.rs`, opt-in): every system's cores from
  nixpkgs load and list the extensions Chanoma sends them; gambatte,
  SameBoy and mGBA play a Game Boy game the test writes, whose screen
  turns black while a direction is held, through whole sessions with the
  controller state, and carry on from their saved state. On 2026-10-31
  with nixpkgs' cores of that date: all three passed (mean brightness
  249–252 untouched, 3 with Right held).
- **In a VM** (`nix build .#aizome-checks.chanoma`): the packaged app in an
  Aizome session, started by the module's autostart, with a uinput
  controller that looks like Microsoft's Bluetooth one and nothing else.
  It checks:
  - the controller found and held (another reader can't grab it);
  - a game opened from Home by the controller, and its picture, as the
    compositor captures it, darkening while Right is held (on
    2026-10-31: 103, then 39, then 103 again);
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
  - stopping the unit mid-game, as logging out does, keeping it.
- **Live** on the X1 Carbon, under a headless Aizome compositor of its
  own beside the desktop (release build, gambatte, generated videos):
  Home, a game shown through the compositor at the Game Boy's shape, the
  pause menu over its picture, *Keep playing* with the picture of where it
  was left, a show's episodes playing on by themselves and Home after,
  PBS KIDS loading in kiosk Chrome and Chanoma coming back when it
  closed, and SIGTERM mid-game keeping the state.

Not yet checked: a physical controller (Bluetooth pairing, and the
mappings of real pads: Xbox, PlayStation, Switch Pro, 8BitDo), the
Bluetooth pairing screen against BlueZ, DRM services (Netflix and the
like) in Google Chrome, sound, the ISO, or a television. These need doing
before calling any of it tested on hardware.

## Limits

- Within a service, its own links can lead elsewhere on the web: kiosk
  mode hides the address bar but doesn't fence a page in. Chrome's
  enterprise policies (`URLAllowlist`) would, machine-wide.
- DRM-protected services need Google Chrome; nixpkgs' Chromium has no
  Widevine.
- No hardware-rendered cores; no disc swapping for multi-disc games; no
  touch screens (the DS).
- Adding a website needs a keyboard and a mouse (its address is typed).
  Choosing folders doesn't: see Choosing folders above.
