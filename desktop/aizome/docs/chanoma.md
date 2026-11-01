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
| `app/` | The program: screens, input, home shelves, playing, settings, time |
| `ui/` | What's drawn: a 1920 × 1080 stage scaled to the screen |

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
The overlay over the page (the Home menu, time warnings) is a layer
surface on the overlay layer, which Aizome draws above a focused
full-screen window; it takes no keyboard focus, and only the menu takes
the pointer.

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
  screen, and a grown-up's grant; bedtime.
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
- Settings that need typing (a website's address, choosing folders other
  than `~/Games`, `~/ROMs` and `~/Videos`) need a keyboard and a mouse.
