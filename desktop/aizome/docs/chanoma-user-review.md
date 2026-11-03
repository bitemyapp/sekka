# Chanoma: two end users' reviews

On 2026-11-02 two reviewers who hadn't seen Chanoma's code, documents or
history tried it as end users and wrote down what they found. One played
a parent who has never used anything like it; the other a long-time
Kodi, Plex, Batocera, RetroArch, ES-DE and LaunchBox user. Their reports
follow as they wrote them, with screenshots named rather than linked.
The last section says what's known so far about the problems they
raised. Both reviewers were Claude agents, each started with only the
brief below.

## How they tried it

- **The build**: Chanoma and the Aizome compositor at `692fc2a2` (the
  code of `b3e79d22`), release builds made on wx-workstation. The folder
  changes under way since (*Settings › Folders*, *Suggested*) aren't what
  they saw.
- **Where**: on the X1 Carbon (Gen 14), each reviewer on a headless Aizome
  compositor of their own at 1920×1080, with a private home, settings,
  D-Bus and runtime folder, apart from the desktop. They pressed keys and
  took screenshots through the compositor's IPC. There was no controller
  (the keyboard stood in, so the hints named keys), no sound, and no
  Bluetooth (the system bus was out of reach).
- **The library**: 846 stand-in games, empty files named from
  libretro-database over eight systems (NES 255, Super NES 205, Game Boy
  Advance 110 zipped, Game Boy 85, Game Boy Color 66, Mega Drive 52,
  PlayStation 40 as cue sheets and tracks, Atari 2600 33), which don't
  start. Six that do: Nova the Squirrel (NES, GPL-3.0), µCity (Game Boy
  Color, GPL-3.0+ and CC BY-SA 4.0), the 240p Test Suite for the NES, Game
  Boy and Game Boy Advance (GPL-2.0), and Chanoma's own test game. The
  engine's test videos: a film and a show of three episodes. The free
  cores of the preview (bsnes, gambatte, mednafen_pce_fast, mesen, mGBA,
  PCSX ReARMed, SMS Plus GX, Stella), so no Mega Drive core. `~/Games`
  and `~/Videos` were links to the library. The streaming tiles opened
  the real sites, signed out.
- **The brief**: how to work the test TV and what was on it (the above);
  not to read the code, documents, history or pull requests; not to count
  a stand-in failing to start as a fault, though how the TV says so
  counts; to stop at about 150 screenshots. The parent took 38 minutes,
  the other 26, about 140 screenshots each, which stayed on the test
  machine.

## The parent who has never used one

I tested Chanoma as a parent who has never used Kodi, Plex, emulators or anything like them, and has a friend-built box under the TV.

### Verdict

The parental side works well. Setup is plain-spoken, the time limits and bedtime enforce themselves, and when time runs out the kids get a kind, clear screen. Watching films and shows works well too. But I would not hand this to my kids yet, for three reasons:

- **Every NES game froze the TV** on a black screen that no button could leave. I had to switch the TV off and on three times.
- **One press in Settings quietly removed my whole games library**, with no "are you sure?".
- **Game tiles are just words.** A child who can't read can't find their game.

### What worked well

- **First run:** a clear welcome, then "Choose a PIN" ("Four to eight presses of these buttons. Nobody sees which"). Typing a different PIN to confirm gave "They didn't match. Try again". After setup a "Ready for the kids" note appeared.
- **Time:** "Monday to Friday" and "Saturday and Sunday" limits, Bedtime and Wake up are easy to set. The kids' screen always shows "60 min left".
- **Bedtime:**
  - Kids' mode at 22:40 showed "It's bedtime – Sleep well. Everything will be here in the morning, from 07:00."
  - "Ask a grown-up", then the PIN, offered 15 minutes, 30 minutes, an hour, or "Carry on as a grown-up". I liked this a lot.
  - When the 15 minutes ran out, uCity was stopped cleanly and bedtime came back.
- **Choosing games:** each system has "Let kids play all 69" and "Keep all 69 from kids", plus per-game switches. The shoulder buttons jump a letter at a time, and the screen says so. My choices survived the library being removed and added back.
- **Wrong PIN:** "That's not it", then after five misses "Too many tries. Wait 0:26". Grown-up mode locks itself after 15 minutes.
- **In-game menu:** "Keep playing / Start again / Stop playing", and Home brings up the same menu. "Keep playing" on the home screen shows a picture of where you left off.
- **Video:** a show opens to its episode list, playback has a clear title and progress bar, and A pauses. B gives "Keep watching / Stop watching".
- **PBS KIDS:** opens full screen with "Press Home… to come back", and Home gives "Keep watching / Go home".
- **A game that won't start** (a PlayStation one) gave a proper "This game didn't start" screen with a "Back home" button.

### Problems, most serious first

**1. NES games freeze the TV with no way out (bug, critical).**
- I tried Metroid (a stand-in), Nova the Squirrel and the NES 240p Test Suite (both real games). Each time the screen went dark and stayed dark for 10–45 seconds.
- B, Home and Start did nothing. The only fix was switching the TV off and on.
- The Game Boy, Game Boy Color and Game Boy Advance games started fine.
- The log says "Starting(…)" and then nothing more, so I can't tell whether this machine lacks its NES player. Either way, a kid is stranded; I expected a timeout and the "didn't start" screen.
- These games also showed up in "Keep playing" afterwards even though they never ran.
- Screenshots:
  - 223426-063-nova-wait2.png
  - 230101-136-nes-240p-escape.png
  - 223439-064-restart2.png

**2. One press removes the whole games folder (bug, serious).**
- Moving into Settings › Games puts the highlight on the folder itself, labelled "Remove this folder". I pressed A expecting to open it.
- It was gone at once: "0 games found", "Systems 0 of 0". There was no confirmation and no undo.
- Settings › Films and shows lands on the same kind of row.
- I got the games back with "Add a games folder…", which is a nice folder picker that said "800 games for 7 systems".
- Screenshots:
  - 225835-121-games-settings.png
  - 225840-122-remove-folder.png
  - 225948-129-watch-settings.png

**3. "Find games on your drives…" found nothing (bug, unsure of cause).**
- It said "No games found", even though the picker then showed Home › Games holding 800 games.
- It may be because of how the test machine's home folder is laid out.
- Screenshot: 225906-124-find-drives-wait.png

**4. Left in Settings changes a value instead of going back to the menu (confusing; it caused real mistakes).**
- I pressed left on Bedtime to get back to the side menu. It changed Bedtime from Off to 22:00.
- My next presses then gave the kids "15 more minutes today" by accident, with a single press and no confirmation.
- B is the way back, but nothing tells you that.
- Bedtime also wraps oddly: left from Off gives 22:00, right from Off gives 18:00, and nothing later than 22:00 is offered.
- Screenshots:
  - 225750-117-add-folder.png
  - 225817-119-bedtime-right-from-off.png

**5. Game tiles are text only (missing, important for young kids).**
- Every game is a grey box with a name and a system label. Only "Keep playing" shows pictures.
- Three tiles are called "240p Test Suite". Advance Wars appears twice, and in grown-up mode Final Fantasy VII appears five times and Crash Bandicoot twice, even with "One of each game" turned on.
- Screenshots:
  - 223118-046-all-games.png
  - 223950-096-ff7-a.png

**6. Nothing helps me judge which games suit kids (missing).**
- Kids start with 0 games allowed. I'm shown 150 NES titles like "Battle Unit Zeoth" and "1942", with no age rating or description.
- In practice the only usable choices are "Let kids play all" or guessing.
- Screenshot: 222900-025-nes-kids.png

**7. The system tabs are hidden (confusing).**
- In All games, the NES / Game Boy / … tabs only change with the shoulder buttons. The d-pad can't reach them, and the hints only mention Choose and Back.
- Pressing up on the top row just jumped the highlight to the first tile.
- Screenshot: 223131-048-up-again.png

**8. Jargon (confusing).**
- "52 without an emulator"
- "PCSX-ReARMed couldn't start this game"
- "Look again: … 8 game players installed"
- "Leave Chanoma – Back to the desktop"
- Folders show as "~/Games" or a long technical path
- I don't know what an emulator or "the desktop" is.
- Screenshots:
  - 224004-098-psx-start.png
  - 225926-127-in-games-folder.png

**9. "Shows on home" is ambiguous (confusing).**
- In Settings › Watch, YouTube, Netflix and Disney+ are switched off, yet they appear on the grown-up home screen.
- I had to work out that this list means the kids' home.
- Screenshot: 230002-131-watch-section2.png

**10. Films are all or nothing (missing).**
- Films and shows says "Kids see everything in it". I can't hide one film from the kids.

**11. Episodes autoplay (polish).**
- Testcard Show ran episodes 1, 2 and 3 back to back, then returned home. I found no setting to stop after one.

**12. "Leave Chanoma" doesn't ask first (polish; partly the test TV).**
- It closed straight away. On this TV that left a blank screen with no way back.

**13. A stand-in that doesn't start gives no message (polish).**
- The Game Boy stand-in Battletoads showed a plain green screen and counted as "Playing". A kid would just think it's broken.
- In "Keep playing" it then appears as a green picture with no name.
- Screenshot: 223739-084-after-toads.png

**14. Can a kid actually use PBS KIDS with a controller? (unsure)**
- The d-pad scrolled the website, but I saw no highlight to show what A would pick. I didn't try A, so I don't know.

**15. Smaller things.**
- There is no "Forgot PIN?" anywhere I looked.
- "Start again" in the game menu has no confirmation.
- I saw no "1 minute left" warning about 20 seconds before the time ran out. One may have shown earlier and I missed it.

### Moments I didn't know what to do or where I was

- On every NES black screen: no spinner, no message, no button that worked.
- On the empty games screen after one press: I had no idea what I'd done or how to undo it.
- In Settings: I didn't know left would change a value and B would go back.
- In All games: I couldn't find how to switch systems until I tried the shoulder buttons.
- Picking games for the kids from a list of names I'd never heard of.

### Suggestions

1. Time out when a game doesn't start (around 10 seconds) and show the existing "This game didn't start" screen. Make Home always work, even while loading.
2. Never put the highlight on a removing action. Ask "Remove Games from Chanoma? Your files stay put" before removing, and offer an undo.
3. In Settings, make left go back to the side menu from a row with arrows, and change values only with A or right. Or show "◀ ▶ change, B back" on screen.
4. Show box art or a screenshot on game tiles. Group duplicates and say what's different ("Europe", "Disc 2").
5. Add an age rating or a "made for young kids" filter, or at least a short description of each game.
6. Show the system tabs in the on-screen hints ("LB/RB: systems") and let up from the top row reach the tabs.
7. Replace jargon: "a game type this TV can't play yet" instead of "emulator"; "the game player" instead of "PCSX-ReARMed"; "Games folder" instead of the path.
8. Add per-film switches for kids, an autoplay switch, a confirmation on "Leave Chanoma", and a "forgot PIN" route.
9. Leave failed games out of "Keep playing", and always show a name under the pictures there.

### Test-rig problems (not Chanoma's)

- No sound, as planned. Pairing said "No Bluetooth here", which was clear.
- The on-screen hints name keyboard keys (Return, Esc, PgUp); "Turn on your controller…" stays at the bottom all the time.
- "Leave Chanoma" goes to a blank screen because this TV has no desktop behind it.
- The games folder sits under a long /tmp path, which may explain the ugly folder name and possibly "Find games on your drives" finding nothing.
- I can't rule out that this machine simply lacks its NES player. The log shows no error.
- I switched the TV off and on three times, all because of the NES freezes. I never needed a full reset. The TV is off now.

### What I covered and what I didn't

**Covered:**
- First run and PIN setup, including a mismatched PIN and backing out
- Time limits, bedtime, extra time from the bedtime screen, and "Carry on as a grown-up"
- Choosing kids' games per system and all at once
- Kids' mode: All games, the system tabs, and the real Game Boy Color and Game Boy Advance games
- The NES games (they froze) and stand-ins on several systems
- The in-game menu (Start again, Stop playing, Home), and resuming from Keep playing
- A show (three episodes) and a film: pause, rewind, stop
- PBS KIDS (looked only, stayed signed out)
- Wrong PIN and the lockout
- Grown-up mode with its extra systems and apps
- The Settings pages for Watch, Games, Films and shows, PIN and lock, and Chanoma
- Removing and re-adding the games folder, Lock now, and Leave Chanoma

About 141 screenshots in all.

**Not covered:**
- YouTube Kids, YouTube, Netflix and Disney+
- Actually changing the PIN
- The Systems on/off page and the demos switch
- Running out of the daily limit outside bedtime (I only hit the bedtime cut-off)
- Weekend limits, the wake-up time, or what happens next day
- The Super NES, Atari and PlayStation games beyond one stand-in
- The Game Boy 240p Test Suite and "Chanoma Test"
- Any real controller, and pointer or mouse use
## The long-time Kodi, Plex and Batocera user

I'm a long-time Kodi, Plex/Jellyfin, Batocera/RetroArch, ES-DE and LaunchBox/Big Box user with a large No-Intro/Redump library, reviewing Chanoma as a living-room box for a family.

### 1. Verdict

The family side of Chanoma is well thought out. The PIN is made of controller buttons, the TV starts in kids' mode, the time limits and bedtime actually take effect, and games resume automatically from where they were stopped. As a frontend for a big curated library it isn't ready. Games show no artwork, there is no search, and the "One of each game" setting still leaves identical tiles: six unlabelled "Final Fantasy VII" tiles and six "Mega Man 2" tiles. Beyond Keep playing / Start again / Stop playing there are no per-game or in-game options. The most serious problem: every NES game I started, including two real ones, hung on a blank screen, and only a power cycle got me out. I'd hand it to a six-year-old for a handful of Game Boy games and PBS KIDS today. I wouldn't replace ES-DE or Kodi with it for my own library.

### 2. What worked well

- **First run and PIN.** Setup asks straight away for a PIN made of controller buttons (d-pad, shoulders, Tab, Space). The buttons are numbered 1–8 so they can be read aloud, and a mismatched confirmation is caught. After repeated wrong PINs it locks you out with a countdown ("Too many tries. Wait 0:26"). This is better than Batocera's kiosk/kid modes or Kodi's master lock. `224139-102-grownups-tile.png`, `224154-103-pin-bruteforce.png`
- **Kids' mode is the default.** Every power-on comes up in kids' mode. Grown-up mode needs the PIN, relocks after 15 idle minutes, and has a "Lock now" option.
- **Time controls.** There are separate weekday and weekend limits, a bedtime, "15 more minutes today", and a "watched and played today" counter. The header shows the time left, and the countdown only ran during kids' use. The bedtime screen is friendly and clear. `223446-064-bedtime-off.png`, `223421-061-restart.png`
- **Kids' home is genuinely curated.** It shows only the games I allowed and only PBS KIDS and YouTube Kids. `223733-079-kids-ucity.png`
- **Automatic game resume.** "Stop playing" saves where you were, and reopening shows "Carrying on where you left off". Keep playing tiles show the last frame of the game. That's nicer than remembering to turn on auto-save-state in RetroArch. `223808-082-ucity-resume.png`
- **Real handheld games started quickly** (about 2–4 s), all pillarboxed correctly: Chanoma Test (GB), uCity (GBC) and 240p Test Suite (GBA).
- **Shows.** A folder of episodes becomes one show with an episode count and a progress bar per episode, and the next episode plays on its own. The playback overlay is clean: Left seeks back and A pauses. `223926-091-show-after-stop.png`, `224019-096-film-a-pause.png`
- **Library hygiene settings.** There are switches for "Hide demos, betas and prototypes" and "One of each game", and a Systems page that lists game counts per system with on/off toggles. Focusing a tile shows its year, genre and number of players. `224252-106-systems-page.png`
- **Home always gets you back from web apps.** In the PBS KIDS browser, Home brings up "Keep watching / Go home".
- **Speed.** At the rig's 0.35 s per press, nothing lagged. Twenty presses down the 150-game NES grid kept up, and "Look again" finished almost instantly.
- **Readable from the sofa.** Large type, a consistent hint bar, and honest messages ("No Bluetooth here").

### 3. Problems, most serious first

**1. NES games hang forever on a blank screen with no way out (bug, serious).**
- **What I tried:** Balloon Fight (a stand-in), Nova the Squirrel and 240p Test Suite (both real NES games).
- **What happened:** All three went to a plain dark screen and stayed there. B and Home did nothing for about a minute. Only `rig stop`/`start` recovered. The log shows "Starting" and then nothing more.
- **It traps kids:** after the power cycle, Nova was the first tile in Keep playing and had focus, so my next press of A hung the TV again.
- **What I expected:** a "Starting…" card that B cancels, and a timeout ending in "This game couldn't start". RetroArch and ES-DE report a failed load and drop you back in the menu.
- **Caveat:** I can't rule out a missing NES emulator on the test TV. Even so, a game that can wedge the TV with no escape is Chanoma's problem.
- **Screenshots:**
  - `223410-060-still-stuck.png`
  - `223602-070-nova-wait.png`
  - `223628-072-restart2.png`
  - `224847-137-nes-still-hung.png`

**2. A game that fails silently looks like it's playing (bug).**
- The Acrobat Mission stand-in (SNES) showed a black screen with no message. It then sat in Keep playing as a black thumbnail tagged NEW.
- A child would just see a broken TV.
- `223202-047-after-wait.png`, `223220-049-stopped.png`

**3. Multi-disc sets and regional versions show up as identical, unlabelled tiles, even with "One of each game" on (bug or missing).**
- **What I saw with it on:** the PlayStation tab has six "Final Fantasy VII" tiles, three each of Symphony of the Night and Crash Bandicoot, and two of Gran Turismo. The NES tab has three "Balloon Fight" and six "Mega Man 2".
- **No way to tell them apart:** tiles show no disc number, region or revision, and one focused FF7 tile had no caption at all.
- **The switch does work on totals** (NES 200→150, PlayStation 40→24), but the visible duplicates stay.
- **What I expected:** ES-DE and Batocera use .m3u files to show a multi-disc game as one entry, and LaunchBox merges discs and shows region and version. With no disc swap in the in-game menu either, multi-disc PlayStation games look unplayable past disc 1.
- **Screenshots:**
  - `224521-122-one-of-each-focus.png`
  - `224625-127-psx-dedup-on.png`
  - `224633-128-ff7-caption.png`
  - `223517-067-nes-down20.png`

**4. No way to search for a game (missing).**
- Typing letters or "mega" in the grid did nothing, and Y did nothing.
- In the grid, RB switches systems. In the settings lists, RB jumps a letter. So in the grid there is no letter jump at all.
- Reaching Nova among 150 NES games took 23 presses of Down. The grid shows only about 8 titles per screen.
- There is no sorting (by year, last played or players), no favourites and no collections.
- ES-DE, Kodi and Big Box all offer search plus a letter jump.
- `223526-068-nes-n-row.png`, `224645-130-grid-type.png`

**5. No artwork for games (missing).**
- Every game tile is a text card. I saw no way to use existing scraped media or a scraper.
- For anyone with a curated, scraped library this is a big step down from every frontend I know.
- `223237-051-all-games.png`

**6. No per-game options and a minimal in-game menu (missing).**
- The in-game menu has only Keep playing, Start again and Stop playing.
- Missing: save/load slots, disc swap, emulator choice, aspect ratio or shaders, controller remapping, and per-game settings.
- X on a game tile simply launches it, where I expected a context menu. Start and Y did nothing.
- `223213-048-game-menu.png`, `223309-055-x-button.png`

**7. Stopping a video partway doesn't leave a way back into it (missing; resume unconfirmed).**
- A half-watched episode or film never appeared in Keep playing.
- After stopping a film, focus jumped to uCity, and my next A launched the game.
- Reopening Episode 3 (tile showed about half watched) and the film (stopped at about 0:03) both started at 0:00. These clips are 6–8 s, so a minimum-resume threshold might explain it. I'm not sure.
- Kodi and Plex offer "Resume from…" and a Continue Watching / On Deck row.
- `223918-090-stopped-watching.png`, `223939-092-ep3-resume.png`, `224106-100-film-resume2.png`

**8. Kids can see every film in a video folder (missing).**
- The videos setting says "Kids see everything in it". I found no grown-up-only folder and no per-title or rating control.
- Plex and Jellyfin offer managed users with rating limits.
- `224655-131-watch-settings.png`

**9. Streaming sites can't really be used with a controller (confusing or missing).**
- On PBS KIDS, Down scrolled the page a little, Right did nothing visible, and there's no focus ring or cursor. B did nothing.
- The "Confirm your local station" box needs a click.
- `224402-114-pbs-nav.png`, `224410-115-pbs-b.png`

**10. Keep playing fills with junk you can't remove (polish/missing).**
- It collects games that never ran, black thumbnails, and PBS KIDS after a grown-up visited it.
- No button on a tile removed it.
- `224239-105-keep-y.png`, `224743-135-restart-kids.png`

**11. Allowing games for kids is one tick at a time (polish).** You go system by system, pressing A on each game, with RB to jump letters. It works, but there's no bulk option such as by genre, rating or a whole folder.

**12. "Shows on home" is mislabelled (confusing).** The Watch settings list YouTube, Netflix and Disney+ as off, but the grown-up home shows all of them. The switches only control the kids' home. `224711-133-watch-pane.png`, `224423-117-back-from-pbs.png`

**13. "Find games on your drives" says "NO GAMES FOUND" (confusing).** It says this while 800 games are already in the library. It presumably means "nothing new". `224311-108-find-drives.png`

**14. In the folder picker, B means "Up" (confusing).** At Home, pressing B went to the folder above home and switched the sidebar to "This computer". It took many presses to get out. `224325-110-after-esc-picker.png`

**15. "Leave Chanoma" exits immediately with no confirmation (polish).** `224734-134-leave.png`

**16. Small polish items.**
- At bedtime the header says "No time left" when the real reason is bedtime.
- The time left is written "1h 30m left" in one place and "89 min left" in another.
- The "Turn on your controller…" banner sits on every screen.

### 4. Moments I didn't know what to do or where I was

- **The NES hangs.** A blank screen where no button did anything.
- **Right after setup the home was "It's bedtime".** My single press on Bedtime had picked a time that had already passed. That's fair behaviour, but setup could warn that it will lock immediately.
- **Six identical FF7 tiles.** I couldn't tell which one was disc 1.
- **Inside PBS KIDS.** No visible focus, and only Home did anything.
- **Watch settings.** I couldn't tell whose home "Shows on home" meant.
- **After stopping a film.** Focus was on a game, and my next A launched uCity.
- **X on a tile started the game** when I expected options.

### 5. Concrete suggestions

1. **Make launches safe.** Show a "Starting *title*…" card that B or Home cancels. Time out with a plain error message, don't add failed games to Keep playing, and detect games stuck on a black screen.
2. **Fix duplicates and discs.**
   - Group "(Disc N)" files and honour .m3u, with disc swap in the in-game menu.
   - When "One of each game" is on, add a region priority setting.
   - Show region and revision when a tile is focused.
3. **Add finding tools.**
   - Search with an on-screen keyboard, and accept typing from a real keyboard.
   - Add a letter jump in the grid (triggers, or LB/RB once tabs move elsewhere).
   - Add sorting (name, year, last played, players) and favourites.
4. **Use existing artwork.** Read ES-DE, Batocera or LaunchBox media and gamelists, or libretro thumbnails, and keep the text card only as a fallback.
5. **Add a per-game menu on X or Y.** Favourite, hide, allow for kids, remove from Keep playing, start fresh, save slots, emulator choice.
6. **Video.**
   - Grown-up-only folders or per-title kids visibility.
   - A resume prompt, and half-watched videos in Keep playing.
   - Return focus to the tile you came from.
7. **Browser apps.** Add a d-pad focus ring or virtual cursor, and map B to browser back.
8. **Bulk kids' rules.** Allow by genre, rating, system or folder.
9. **Wording fixes.**
   - "Shows on home" → "Kids can watch".
   - "No new games found (800 already in your library)".
   - Confirm "Leave Chanoma".
   - Make B leave the folder picker.

### 6. Test-rig problems, separate from Chanoma

- No sound, no Bluetooth, keyboard-key hints and a browser hitting real websites are all as described.
- The folder paths in settings are long rig temp paths, and "This computer" opened the rig's own folders.
- "Leave Chanoma" shows a blank screen because the rig has no desktop.
- **NES hangs:** the log showed nothing after "Starting", so I can't say whether NES is broken in Chanoma or missing on this TV.
- **Restarts:** I power-cycled four times: after the Balloon Fight hang, after the Nova hang, after accidentally relaunching Nova, and after "Leave Chanoma". The final NES hang ended with `rig stop`. I never needed `rig reset`.
- **My own tooling, not the rig:** my image viewer dropped the first ~47 screenshots partway through. So the details of first-run setup and the kids' allow-list screens are from my notes, not re-checked images.

### 7. What I covered and didn't

**Covered:**
- First run: PIN creation (mismatch caught), time limits, bedtime, kids' allow-lists for NES, Game Boy, GBC and GBA.
- Kids' home; grown-up mode and the PIN lockout.
- The games grid and system tabs; "Hide demos" and "One of each game"; the Systems page; the folder picker; "Look again"; "Find games on your drives".
- Real GB, GBC and GBA games played, stopped and resumed; the SNES stand-in; three NES launches (all hung); browsing PlayStation.
- Show and film playback, pause, seek, autoplay of the next episode and progress bars.
- PBS KIDS in the browser; the "Pair a controller" screen; the PIN & lock and Chanoma panes; Leave Chanoma.
- About 140 screenshots.

**Not covered:**
- YouTube Kids and the other streaming tiles.
- The limit running out during play; "15 more minutes today"; the 15-minute auto-lock firing; changing the PIN.
- Adding or removing folders; Atari 2600 games; how the "52 games need a player that isn't [installed]" look and behave.
- "Start again"; any NES game actually running.
- A real controller and its on-screen hints.
- Performance with real artwork.
## What's known about what they found

What could be checked without changing anything, on the day; the rest
are requests, not yet looked at.

- **NES games don't start, and nothing gets out of it** (both, every NES
  game tried, two of them real). Game Boy, Game Boy Color and Game Boy
  Advance games started. The log stops at `Starting("240p Test Suite")`,
  before the sound a game sets up next, so it's stuck loading the core
  (mesen, `libretro-mesen-0-unstable-2026-04-20`, the preview's) or the
  game in it. Whether that's mesen in this setup or Chanoma isn't known
  yet. Either way, a start that never ends has no way out: there's no
  timeout, and Home and Back did nothing while it started.
- **A stand-in that doesn't start can look as if it plays** (both): a
  core given an empty file may show a black or green picture rather than
  refuse it, and Chanoma says *This game didn't start* only when the core
  refuses (as PCSX ReARMed did). Such a game then sits in *Keep playing*.
- **One press removed the games folder** (the parent): a folder's row
  was *Remove this folder*, with no question asked. Since then (not what
  they saw) a folder's row opens its own page, where removing is a row
  of its own, and asks first.
- **The duplicates under *One of each game*** (both) come from tags it
  keeps as part of a game, as it keeps `(Disc 2)`: releases and editions
  (`(Virtual Console)`, `(e-Reader Edition)`, `(Animal Crossing)`,
  `(Capcom Town)`, `(iam8bit)`, `(Mega Man Legacy Collection)`, `(EDC)`),
  and demos in Japanese (`(Taikenban)`, `(Otameshi Disc)`, `(Test Drive
  Disc)`), which *Hide demos* doesn't know. Final Fantasy VII's discs are
  games of their own unless a playlist (`.m3u`) joins them, and the test
  library's database names gave it a `(Disc 2)` and a `(Disc 2 of 3)`.
  The three *240p Test Suite* tiles are three systems' games.
- ***Find games on your drives* found nothing** (both) because the test
  library's `~/Games` was a link, which the files index doesn't follow
  (`docs/files-index.md`); the games came from the link added as a
  folder. Saying *No games found* beside 800 games still misleads.
- **Videos didn't carry on** (the Kodi user): by design for films under
  five minutes (`resume_point`), and the test videos run 6 to 8 seconds.
  A half-watched episode not in *Keep playing* wasn't checked.
- ***Shows on home*** (both) are the kids' Home; grown-up mode shows
  every service. The wording, not the behaviour, misled.
- **Back in the folder picker** (the Kodi user) goes up a folder, from
  home to the computer's folders. With the folder changes, the picker
  opens on *Suggested*, where Back goes to the places.
- **Left in the settings** (the parent) changes a choice on a row that
  has one, and goes back to the sections on any other. Bedtime's choices
  run 18:00 to 22:00 and wrap.
- **No pictures on game tiles** (both): the test library had no box art.
  Chanoma takes a picture beside a game (`Game.png`, or in `media/`,
  `boxart/`, `images/`, `covers/` or libretro-thumbnails'
  `Named_Boxarts/`) named as the game is; ES-DE's scraped media and
  `gamelist.xml` aren't read.
- **Not looked at yet**: search, sorting and favourites; a menu for each
  game (save slots, swapping discs, a core to choose); per-film kids'
  switches; age ratings or descriptions to choose kids' games by; a
  focus to see on streaming pages; stopping after one episode; asking
  before *Leave Chanoma*, *Start again* or more time; a way back from a
  forgotten PIN; plainer words for *emulator*, a core's name and *the
  desktop*; the system tabs reached with the d-pad and named in the
  hints.

## Since

What the reviews led to, the next day (`docs/chanoma.md` has each):

- **NES games**: it was Chanoma. It gave a core its video, sound and
  input callbacks before `retro_init`, which libretro doesn't promise;
  Mesen keeps them in what `retro_init` makes, and crashed, taking
  Chanoma with it (the core dumps said so). The callbacks go in
  libretro's order now, the tests' core loses one given early as Mesen
  did, and the NES's 240p Test Suite plays on the test TV.
- **Removing** a folder or a website asks first, *Keep it* ready to
  press.
- **Pictures on every game**: its box beside it (`gamelist.xml`,
  Batocera's names), else fetched by name from libretro-thumbnails and
  kept, else where it was left, else a sprite of squares made from its
  title.
- **Search**, from Home and the games: Y, or typing on a keyboard.
- **Films and shows for kids**, one by one, and new ones waiting for a
  grown-up if wanted.
- **A forgotten PIN** chosen again after signing in with the password of
  the computer's account.
- **The duplicates** and the rest are left as listed above.
