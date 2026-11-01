# Demos and tutorials

Ideas and scripts for the Sekka channel on YouTube,
[@sekkacomputer](https://www.youtube.com/@sekkacomputer), in two playlists:

- **Demos** show one thing Sekka or Aizome does, in a minute or less, with
  captions and no narration. The 70-second
  [Sekka × Aizome](https://www.youtube.com/watch?v=jF4OwL1GbxY) video opens
  the playlist.
- **Tutorials** show how to do something from start to finish, in two to
  six minutes, narrated.

Every claim in a script comes from this repository's READMEs and docs, cited
beside it. When a feature, key or limitation changes, change its script with
it, as the site's pages are changed.

## The playlists

| Playlist | Title | Description |
| --- | --- | --- |
| Demos | Sekka demos | Short looks at Sekka 雪花 and Aizome 藍染, its desktop: one thing each, in a minute or less. Sekka is early and experimental; https://sekka.computer says what doesn't work yet. |
| Tutorials | Sekka tutorials | How to try, install, use and look after Sekka, start to finish. Each video says what doesn't work yet. https://sekka.computer |

Order each playlist as below: the demos from what you see first to what you
find later, the tutorials in the order someone new to Sekka needs them.

## Recording

- Record in Aizome itself. **❖⇧5** opens the Capture menu: **Record the
  Screen ▸** or **Record an Area ▸**, each with desktop audio, the
  microphone, or both. **⌥Print** starts a recording and stops it, and so
  does a click on the red dot in the systray, which counts the time.
  Recordings are encoded on the GPU
  ([shell.md, Screen recording](../desktop/aizome/docs/shell.md#screen-recording)).
- Record at 3840 × 2160: a 4K display, or a 1920 × 1080 layout at scale 2.
  YouTube encoded the hype video's first upload, at 1080p, to AV1 at
  750 kb/s; uploaded at 2160p, it got 2160p and 1440p encodes as well.
- Add no grain, noise or film effects. YouTube's encodes turn noise that
  changes every frame into blocks: the hype video lost its grain for that.
- Use an account made for recording: sample files, no real names, mail,
  Wi-Fi networks or SSH keys on screen, and Do Not Disturb on (the palette's
  **Do Not Disturb** action). Genkan's GitHub step shows public keys; use a
  throwaway account there or skip it.
- Say where it was recorded, as the README's screenshots do: the machine,
  or "in a test VM, rendering in software".
- Use Night unless the video is about themes.
- Try every step in the build you're recording first. `apps.md` describes
  some apps ahead of what is built, and the scripts below leave those out.
- Write captions and titles as Sekka writes everything
  ([brand README, Voice](brand/README.md#voice)): sentence case, no
  exclamation marks, no emoji, menu paths joined with › (**Yukimi ›
  Updates › Update**), keys as Aizome's docs write them (**❖Space**,
  **⌃❖↑**), and what doesn't work said plainly.
- Name videos for what they show: "Tabs from any apps · Aizome", "Install
  Sekka with Genkan".
- End on https://sekka.computer, with the playlist's next video in the end
  screen.

## Demos

Each demo is a list of shots. The quoted line under a shot is its
caption.

### D1. Tabs from any apps

*40 s. Set-up: Hako, Sumi and Hotaru in windows of their own on one Desk,
overlapping. Sources: [README][r-aizome], [Keys][keys].*

1. Click Sumi. Its tab turns amber.
   > Every window in Aizome wears a tab only as wide as its title.
2. Drag Hotaru's tab among Sumi's and let go.
   > Drop one tab among another window's, and the windows stack into a group, from any apps.
3. Drag Hako's tab into the group, between the other two.
   > The tab goes where you drop it.
4. Press **❖⇧]**, then **❖⇧[**, with the keys on screen.
   > ❖⇧[ and ❖⇧] step through a group, as ⌘⇧[ and ⌘⇧] do on a Mac.
5. Hold **Shift** and slide a tab along the top edge.
   > Shift slides a tab along the top edge.
6. On Hotaru's tab, press **⌃⇧❖S**.
   > And ⌃⇧❖S takes it back out.

### D2. The whole menu, wherever the pointer is

*35 s. Set-up: Sumi with a paragraph of text, a sentence selected.
Sources: [README][r-aizome], [shell.md, Menus][menus].*

1. **❖-right-click** over the text. Sumi's whole menu opens at the pointer.
   > ❖ and a right-click open the app's whole menu at the pointer, anywhere. That's NeXT's idea.
2. Type "quo". The menu narrows to Format › Quote.
   > Type to filter a menu. Submenus come with their paths.
3. Open Format from the menu at the pointer and drag its title strip away.
   Pin the palette.
   > Drag a menu by its title and it tears off into a palette. Pinned, it stays when you switch apps.
4. Sumi's app menu › Services › New Note with Selection. Fusen opens with
   the sentence.
   > Services act on the selected text: a note, a search, a sum.

### D3. A palette for everything

*45 s. Set-up: Sumi in front. Beforehand, run Edit › Find from the menu bar
three times, so the shortcut coach shows. Sources: [shell.md, Command
palette][palette].*

1. Press **❖Space** and type "find".
   > ❖Space searches every menu of the app in front, with each command's keys.
2. Arrow down to Edit › Find. The detail pane reads "You've run this from
   menus 3 times this week. The keys above do it without leaving the text."
   > Use a command from the menus often, and the palette teaches you its keys.
3. Type "caps" and run Keyboard › Remap Caps Lock. Settei opens at it.
   > Settings are in it too, and open where the option is.
4. Type "1920 * 1080". The first row is the answer; press **Return**.
   > Arithmetic answers in the first row. Return copies it.
5. Press **⇥** through Windows, Files and Apps. On a file, press **❖K**:
   Open With ▸, Show in Hako, Copy Path.
   > Windows, files and apps too. ❖K shows what you can do with each.

### D4. The Toolchest, Desks and the Deskbar

*55 s. Set-up: Desks named Mail, Build and Write; a few apps open; a
notification filed in the Deskbar. Sources: [README][r-aizome],
[Toolchest][toolchest], [Menu bar][menubar], [Deskbar][deskbar].*

1. Press **❖⌥Space**. Open Applications ▸, Places ▸, Desks ▸ and System ▸
   in turn.
   > The Toolchest, from IRIX: apps, places, Desks and the system.
2. Click Build, then Write, in the menu bar's Desks strip. Press **⌃❖↑**.
   > Desks keep work apart. ⌃❖↑ shows them all.
3. Pan to the Deskbar: each app with its windows, "3 new" under one of them.
   > The Deskbar, from Haiku, lists each app with its windows, and files each notification under the app that sent it.
4. Drag the dotted grip along its top to the left half of the screen. The
   outline reads Drag here to move, then Release to move here.
   > It stands at the right or the left.
5. Drag its inner edge until it shows icons only. Right-click the grip and
   choose Standard.
   > As wide as you like, or icons only.
6. Drag a file from Hako onto Drop to park. Switch Desks and drag it out
   into a folder there.
   > The Shelf parks a drag halfway, while you find where it goes.

### D5. Super is the command key, on every keyboard

*40 s. Set-up: a laptop, and a PC keyboard (Alt beside the space bar) to
plug in. Sources: [README][r-aizome], [apps.md, Settei][settei].*

1. **❖C** a file in Hako, **❖V** it into another folder, **❖Z** in Sumi.
   > ❖ is the command key in every app, as ⌘ is on a Mac.
2. Plug in the PC keyboard. Aizome offers to set it up.
3. Press the key beside its space bar. Settings suggests swapping Alt and
   Super on that keyboard alone.
   > Each keyboard keeps its own modifier keys, so ❖ sits under your thumb on a Mac keyboard and a PC keyboard alike.
4. Type on both.
   > Layouts and shortcuts are the same on every keyboard.
5. Caption over the end: "Under an input method, such as IBus or Fcitx 5,
   keyboards don't keep their own keys yet."

### D6. One file chooser

*30 s. Set-up: a browser and Zed open, some documents in ~/Documents.
Sources: [README][r-aizome], [apps.md, The file chooser][chooser].*

1. Open a file from the browser. Aizome's chooser comes over its window.
   > Browsers, Electron apps, Zed and Flatpaks open and save files in Aizome's own chooser.
2. Show its sidebar, path bar and the browser's file types. Press **⇧❖O**.
   > Files in miniature, with the app's filters and options.
3. Save from Zed, typing `~/Documents/plan.md` as the name.
   > A name can be a path.
4. Caption over the end: "Qt apps, and GTK 4 apps that don't ask the
   desktop portal, still show their own dialogs."

### D7. Hotaru and fish

*40 s. Set-up: a Desk whose folder is a Git repository (right-click the Desk
› Set Folder…). Sources: [README, Aizome][r-aizome] (Hotaru), [README,
Genkan][r-genkan] (fish), [Keys][keys].*

1. Press **❖Return**. Hotaru opens in the Desk's folder.
   > ❖Return opens Hotaru, Aizome's own terminal, in the Desk's folder.
2. Run `ls`, then `cd` with part of a folder's name.
   > fish, set up as Omarchy sets it up: eza for ls, zoxide behind cd.
3. **Ctrl + R**, then **Ctrl + Alt + F**, then **Ctrl + Alt + L**.
   > fzf through your history, your files and the Git log.
4. Caption over the end: "Hotaru doesn't do synchronized output or the
   Kitty graphics protocol. foot is installed for programs that need them."

### D8. Dictation that stays on your computer

*40 s. Set-up: Koe on, with the Base model; Sumi in front. Sources:
[koe.md][koe], [Privacy][koe-privacy].*

1. Press **❖⌥D**. The microphone in the systray turns red.
   > ❖⌥D in any app, and speak.
2. Say a sentence with "Aizome" in it. Press **❖⌥D** again. It's typed into
   Sumi.
   > Press it again, and the words are typed where you were.
3. Open Koe's Vocabulary and Teaching page (**❖2**), at the Sekka pack.
   > Its Sekka pack knows how to spell Aizome.
4. "Recognised by Whisper on this computer, on the graphics card if there is
   one. The audio is never written to disk."
5. Caption over the end: "No words appear while you speak yet: Koe types
   when you finish."

### D9. Ctrl+Alt+Del that always answers

*45 s. Set-up: a few apps open, one of them stopped with `kill -STOP` on
its process. Sources: [recovery.md][recovery].*

1. Over a full-screen app, press **Ctrl+Alt+Del**. The overlay opens at once.
   > Ctrl+Alt+Del opens Aizome's recovery from anywhere: over full-screen games, keyboard grabs and the lock screen.
2. The list: every app with its processor and memory, one marked Stopped.
3. Select it and press **Delete**.
   > Force Quit ends the app and everything it started.
4. Press **S**. The shell restarts; the windows stay.
   > Restart the shell, and your apps keep running.
5. Caption over the end: "A hung X11 app isn't marked Not responding."

### D10. Install Sekka in two minutes

*60 s, sped up, in a test VM. Sources: [README, Genkan][r-genkan], [Before
you install][r-before], [applications.md][applications].*

1. The boot splash, then the live Aizome session with Genkan open.
   > Genkan, Sekka's installer.
2. The disk page, with the encryption switch.
   > It takes a whole disk. LUKS2 encryption is a switch.
3. Account, then Desktop: Aizome selected, Tatami beside it.
   > Aizome, Tatami, Plasma, GNOME, Hyprland or Xfce, or several.
4. Applications: search, tick two, Firefox already ticked.
5. Keyboard: press the key beside the space bar. Genkan recommends swapping
   Alt and Super.
   > ❖ belongs under your thumb.
6. Review, while the progress line runs.
   > Genkan prepares the system while you review it.
7. Restart: the boot splash and its rail, the login screen, Aizome's
   welcome.
   > There's no dual boot. Keep your files elsewhere.

### D11. Yukimi: your system, without a text editor

*45 s. Sources: [README, Yukimi][r-yukimi], [Yukimi's README][yukimi-readme].*

1. Installed, grouped by why each thing is there.
   > Everything installed, and why.
2. Discover: search, then install a package just for you.
   > Try a package, install it for you, or for everyone.
3. History: open a generation and see what changed.
   > Every change is a generation you can go back to.
4. Storage: the heaviest packages, then clean up.
5. Updates › Update.
   > Updates download what Sekka built and tested. Nothing is compiled on your computer except NVIDIA's driver.
6. Caption over the end: "Generations aren't backups: they bring back an
   earlier system, not earlier documents."

### D12. Tatami, for people who would rather tile

*45 s. Sources: [README, Tatami][r-tatami].*

1. Choose Tatami on the login screen. Windows tile beside the bar.
   > Tatami: Hyprland, a bar, a launcher and menus, driven from the keyboard.
2. **Style › Background settings**: fill, fit, tile, per display.
3. **Super + Ctrl + T**: Activity, sorted by energy.
   > Each application with all its processes together.
4. **Super + Ctrl + I**: the coffee cup in the bar.
   > Keep Awake, for long builds and downloads.
5. On a laptop, close the lid with an external display connected.
   > Close the lid, and its windows move to the other screen.

### D13. Night, Day and High Contrast

*25 s. Sources: [apps.md, Settei][settei], [Look][look].*

1. Settei › Appearance: Night, then Day, then High Contrast. Everything on
   screen changes at once.
   > Night is the default. Day is IRIX's nod. High Contrast is for low vision.
2. Choose Auto. Its line names today's sunrise and sunset.
3. Focus colour: amber, cyan, rose, lime.

### D14. Aizome's own apps

*60 s, a cut every four or five seconds, each with the app's name and
kanji. Sources: [apps.md][apps], [README][r-aizome].*

1. Shashin 写真: **❖9** to fit, **❖0** for 100%, **F** for the filmstrip.
2. Shiori 栞: **❖F**, hits in yellow.
3. Utsushi 映し: **]** to play faster, pitch kept.
4. Kura 蔵: click into the sunburst, then its suggestions of what to free.
5. Nisshi 日誌: type `level:`, then a suggestion becomes a chip.
6. Myaku 脈: View › Energy Meters….
7. Tenki 天気: the hourly strip and ten days.
8. Sumi 墨: Markdown styled as you type.
9. Settei 設定: the Performance pane, frame times against the 4 ms budget.
10. "Seventeen apps of its own, and Hotaru, its terminal."

## Tutorials

Each tutorial is a list of steps. The quoted line under a step is what to
say.

### T1. Try Sekka from a USB drive

*3 min. You'll need an x86_64 PC with Secure Boot off and a USB drive you
can erase. Sources: [Trying Sekka][r-trying], [Before you
install][r-before], [Kakehashi][kakehashi].*

1. Open sekka.computer/download.
   > An ISO is published only after that exact image has been installed and used in virtual machines. Each one lists its checksum and the revision it came from.
2. Write it with Kakehashi.
   > Kakehashi lists the nightlies, downloads and checks the one you pick, writes it, and reads every byte back. Any tool that writes a raw disk image works too.
3. Boot from the drive. Show the boot menu.
   > The menu also has an older kernel, and a recovery entry that draws in software, without NVIDIA's driver.
4. The splash; press **Esc** for the boot messages, **Esc** to go back.
5. The live Aizome session, Genkan open.
   > Try the live desktop on your computer before you install.
6. "GitHub's Source code archives aren't bootable images."

### T2. Install Sekka with Genkan

*6 min, in a VM, with a cut to real hardware if there's one. Sources:
[README, Genkan][r-genkan], [Before you install][r-before], [Trying
Sekka][r-trying], [applications.md][applications].*

1. Say what [Before you install][r-before] says: the whole disk is erased,
   an internet connection is needed, x86_64 with Secure Boot off,
   generations aren't backups.
2. Disk: choose it; ext4, Btrfs or XFS; RAM-sized swap and tuning, on by
   default. Turn on encryption and choose a passphrase.
   > Remember the disk passphrase. Changing your login password doesn't change it, and Genkan can't recover it.
3. Account, then Desktop.
   > Aizome is selected, with Tatami beside it. Install several and choose at login.
4. Applications: search, tick, and the review of your selection.
   > Proprietary software is allowed by default, with an opt-out.
5. GitHub (optional) and Wi-Fi.
   > Genkan can import your public SSH keys, set your Git identity, and carry this session's Wi-Fi into the installed system.
6. Keyboard: press the key just left of the space bar.
   > If it's Alt, Genkan recommends swapping left Alt and Super. Right Alt stays for accents and symbols.
7. Review, then confirm erasure.
   > Genkan has been preparing the system while you reviewed it. Now it copies it to disk.
8. Restart. Type the passphrase into the splash; log in.
   > Next: your first ten minutes in Aizome.

### T3. Your first ten minutes in Aizome

*5 min. Sources: [apps.md, Annai][annai], [README][r-aizome], [Keys][keys],
[Toolchest][toolchest], [Command palette][palette].*

1. Annai's welcome, page by page.
   > It shows once. Help › Aizome Tour brings it back.
2. The menu bar, left to right: the Toolchest, the app's menus, the Desks,
   the systray, the clock.
3. **❖⌥Space**: open Sumi and Hako from the Toolchest.
4. **❖Return** for Hotaru; drop its tab on Sumi's.
5. **❖Space**: the palette.
6. The Deskbar: windows and notifications.
7. Annai's last page, Make it yours: Appearance, Displays, Network,
   Keyboard.
8. **⌃❖Q** locks. Session › Log Out… asks first, counting down 60 seconds.

### T4. Find, install and remove software with Yukimi

*4 min. Sources: [README, Yukimi][r-yukimi], [Yukimi's README][yukimi-readme].*

1. Overview.
   > The system at a glance: NixOS version, kernel, how fresh Nixpkgs is, how much room the store takes.
2. Discover: search for an application.
   > Broken and unfree packages are labelled before you pick them.
3. Try it without installing; then install it just for you.
   > Just for you needs no password. For everyone asks for it.
4. Installed: find it, grouped by why it's there, and remove it.
   > Yukimi takes out just that entry, even one you wrote into your configuration yourself.
5. Storage: the heaviest packages and older generations; clean up.
6. "Everyday changes don't need you to edit Nix configuration by hand."

### T5. Update Sekka, and go back

*4 min. Sources: [Updating an existing installation][r-updating], [Yukimi's
README][yukimi-readme].*

1. **Yukimi › Updates › Update**.
   > You get Sekka's newest nightly: NixOS, the applications and the AI tools at the versions it built and tested.
2. "Nothing is compiled on your computer except NVIDIA's driver. If space runs short, the update stops before the disk fills and changes nothing."
3. The completion message.
   > Restart when you're ready. Sekka never restarts or signs you out on its own.
4. The same in a terminal: `sekka-update`, then `sekka-update --status`.
5. Updates › Early updates.
   > Early updates follow main as soon as changes are merged, and may build on your computer. `sekka-update --nightly` goes back.
6. History: open the previous generation, see what changed, go back to it.
   > Generations aren't backups. They return you to an earlier system, not to earlier documents.

### T6. Change your desktops after installing

*3 min. Sources: [Updating an existing installation][r-updating], [Prefer a
familiar desktop?][r-familiar], [apps.md, Yukimi][yukimi].*

1. **Yukimi › Desktops**: tick GNOME, choose the default desktop, the login
   screen and the boot splash.
   > Nothing changes until you press Apply.
2. Apply, enter the password; **Restart Now** if it's offered.
   > Adding or removing a desktop doesn't wait for a restart. The login screen lists it after your next one.
3. The same in a terminal: `sekka-update --choose calamares.loginScreen=aizome`.
4. Log out and choose GNOME.
   > With GNOME installed, the login screen is GDM, since GNOME locks its screen only under GDM.

### T7. Set up your keyboards

*3 min. Sources: [README, Aizome][r-aizome], [README, Genkan][r-genkan],
[apps.md, Settei][settei].*

1. "❖ is Aizome's command key, as ⌘ is on a Mac, and it belongs under your thumb."
2. Settei's keyboard settings, Typing › Keys › Modifier keys: Swap left Alt
   and Super. Default puts it back.
   > Right Alt stays for accents and symbols.
3. Plug in another keyboard: Set Up…, then press the key beside its space
   bar. Or Same as Other Keyboards.
   > Keys › Keyboard says whose modifier keys each keyboard uses.
4. Caps Lock as Control, Escape or Compose; the shortcut list.
5. "Under IBus or Fcitx 5, every keyboard types with the keys of the one in use when it began. Keyboards keeping their own keys there is still to come."

### T8. Windows, tabs and Desks in depth

*5 min. Sources: [README][r-aizome], [Keys][keys], [Menu bar][menubar],
[Toolchest][toolchest], [apps.md, Settei][settei].*

1. Tabs: drop one among another window's; **Alt**-drag the one showing;
   **Shift** to slide. **⌃❖S** stacks with the window behind, **⌃⇧❖S**
   takes the tab out, **❖⇧[** and **❖⇧]** step, **⌃⇧❖[** and **⌃⇧❖]** move
   the tab.
2. Right-click a tab for the marking menu: fill, park, split ←/→, close,
   stack, float, to desk ▸. **⌃❖Space** opens it from the keyboard;
   **⌥❖** and an arrow does its verbs.
3. Desks: **⌃❖1…9** and **⌃❖←/→**, with **⇧** to take the window along;
   **⌃❖↑** for all of them.
4. Right-click a Desk › Set Folder…. **❖Return** now opens the terminal
   there, and the Toolchest's Places has it.
5. **❖Tab** for the switcher, **❖`** for this app's next window, **❖H** to
   hide, **❖M** to minimise.
6. Settei › Desktop & Deskbar: stack new windows of the same app, and what
   double-clicking a tab does.

### T9. The terminal: Hotaru and fish

*4 min. Sources: [README, Aizome][r-aizome], [README, Genkan][r-genkan].*

1. **❖Return**. "Programs that need a terminal open in Hotaru too."
2. fish, set up as Omarchy sets it up: Starship's prompt, vi keys, eza,
   zoxide, and fzf on **Ctrl + R**, **Ctrl + Alt + F**, **Ctrl + Alt + L**
   and **Ctrl + Alt + S**.
3. "Your own functions in ~/.config/fish, and a ~/.config/starship.toml, replace Omarchy's. Bash stays your login shell and runs scripts."
4. "Some of Omarchy's shortcuts call tools fish doesn't bring, such as tmux, ImageMagick or ffmpeg, and iso2sd and try need Omarchy itself."
5. Settei › General › Default Apps › Terminal.
   > Hotaru doesn't implement synchronized output, in-band resize or the Kitty graphics protocol. Choose foot here for programs that need them.

### T10. Dictation with Koe

*4 min. Sources: [koe.md, Setting it up][koe-setup], [Using it][koe-using],
[Vocabulary and Teaching][koe-vocab], [Privacy][koe-privacy].*

1. Open Koe from the Toolchest. **Download a Model and Turn On**.
   > The Base model is 148 MB, and its checksum is checked.
2. Speech Model: Tiny, Base or Small, multilingual or English only.
   Recognition: the language or Detect automatically, Translate into
   English, Use the graphics card.
3. **❖⌥D** to start and finish, **❖⌥⎋** to cancel.
   > The microphone in the systray is red while it listens.
4. Typing: into the app, to the clipboard, or kept in Koe.
   > If the window changed while you spoke, Paste Last Transcript is in the systray menu.
5. Vocabulary and Teaching (**❖2**): the Sekka pack, a Personal pack, a
   recorded example.
   > Examples are kept; no model training runs on them yet.
6. "The audio is never written to disk, notifications never contain what you said, and nothing goes to a server unless you set one up."

### T11. When something goes wrong

*5 min. Sources: [recovery.md][recovery], [Toolchest][toolchest],
[apps.md, Nisshi][nisshi], [README, Questions][r-issues].*

1. **Ctrl+Alt+Del**: **W** closes, **Delete** force quits, **R** resets
   input, **S** restarts the shell, **A** reloads Aizome, **L** locks, **O**
   twice logs out, **T** twice opens a text console.
2. "If the compositor stalls for 30 seconds, it restarts on its own. Apps lose their windows when that happens."
3. An app that won't start: Toolchest › System › Application Launches…,
   Show Details, Open in Logs.
4. Nisshi: `level:`, `app:` and `since:10m` in the search; a suggestion;
   **Unlock…** for what your account can't read.
5. "Open an issue on GitHub with what you tried, what happened, and the image version or source revision. Read logs for personal information before you attach them."

### T12. Keep Awake, laptops and displays

*3 min. Sources: [keep-awake.md][keep-awake], [apps.md, Settei][settei],
[README, Tatami][r-tatami].*

1. The battery panel › Keep Awake. The amber cup appears.
   > Also in the palette, and as `aizome-awake on`. It stays on across restarts until you turn it off.
2. "A red cup means the computer may sleep anyway. Turning it off and on again retries."
3. Close the lid with a monitor connected.
   > No sleep: the built-in panel goes dark, and the Desk moves to the monitor.
4. "It still sleeps when you choose Sleep yourself, and a flat battery or overheating still shuts it down."
5. Settei › Displays: drag the displays into place.
   > Changes apply at once, and go back after 15 seconds unless you keep them.
6. In Tatami: **Super + Ctrl + I** and **Super + Ctrl + D**.

### T13. Add Aizome to the NixOS you already have

*3 min. Sources: [Aizome's README, Using it][a-using], [README,
Questions][r-issues].*

1. The flake: `sekka.nixosModules.aizome` and
   `programs.aizome.enable = true`.
2. Rebuild, and choose **Aizome** on the login screen.
3. "Aizome, Tatami and Yukimi each work on NixOS through Sekka's own modules."

### T14. Change an Aizome app and try it in your session

*5 min, for contributors. Sources: [Aizome's README, Developing][a-dev].*

1. `nix develop .#aizome`, `cd desktop/aizome`, `cargo build`.
2. Change a label in Yukimi; run `just replace yukimi`.
   > Its windows close and the new build starts. A failed build leaves the running app alone.
3. "The next launch from the Toolchest is the installed app again. Run the command again to test more."
4. `scripts/headless-session` to run the whole desktop without touching
   yours.
5. "Aizome lives in Sekka's repository, so its changes are Sekka pull requests."

[r-aizome]: ../README.md#aizome-sekkas-own-desktop
[r-genkan]: ../README.md#genkan-welcome-home
[r-tatami]: ../README.md#tatami-a-desktop-under-your-fingertips
[r-yukimi]: ../README.md#yukimi-see-and-change-your-system
[r-familiar]: ../README.md#prefer-a-familiar-desktop
[r-before]: ../README.md#before-you-install
[r-trying]: ../README.md#trying-sekka
[r-updating]: ../README.md#updating-an-existing-installation
[r-issues]: ../README.md#questions-bugs-and-contributions
[a-using]: ../desktop/aizome/README.md#using-it
[a-dev]: ../desktop/aizome/README.md#developing
[keys]: ../desktop/aizome/docs/architecture.md#keys
[look]: ../desktop/aizome/docs/architecture.md#look
[menubar]: ../desktop/aizome/docs/shell.md#menu-bar
[menus]: ../desktop/aizome/docs/shell.md#menus
[toolchest]: ../desktop/aizome/docs/shell.md#toolchest
[palette]: ../desktop/aizome/docs/shell.md#command-palette
[deskbar]: ../desktop/aizome/docs/shell.md#deskbar
[apps]: ../desktop/aizome/docs/apps.md#what-each-app-does
[settei]: ../desktop/aizome/docs/apps.md#settei-settings
[yukimi]: ../desktop/aizome/docs/apps.md#yukimi-software
[nisshi]: ../desktop/aizome/docs/apps.md#nisshi-logs
[annai]: ../desktop/aizome/docs/apps.md#annai-welcome
[chooser]: ../desktop/aizome/docs/apps.md#the-file-chooser-portal
[koe]: ../desktop/aizome/docs/koe.md
[koe-setup]: ../desktop/aizome/docs/koe.md#setting-it-up
[koe-using]: ../desktop/aizome/docs/koe.md#using-it
[koe-vocab]: ../desktop/aizome/docs/koe.md#vocabulary-and-teaching
[koe-privacy]: ../desktop/aizome/docs/koe.md#privacy
[recovery]: ../desktop/aizome/docs/recovery.md
[keep-awake]: ../desktop/aizome/docs/keep-awake.md
[applications]: applications.md
[kakehashi]: ../apps/kakehashi/README.md
[yukimi-readme]: ../apps/yukimi/README.md
