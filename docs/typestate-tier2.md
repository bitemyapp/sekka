# Tier 2 state machines as typestate

Tier 1 (`docs/typestate-tier1.md`) reified the machines whose bugs could
wipe the wrong disk, lose files or leave a system that doesn't boot. Tier 2
does the same for authentication and locking, the password fields they use,
and the confirmation of destructive steps:

- the lock screen and lock-before-sleep;
- the polkit agent and its dialog;
- the greeter;
- secret text fields;
- a person's agreement, which only the kit can make, when someone presses a
  button that agrees to a destructive step.

Every bug is fixed. For each one, the code that made the mistake no longer
compiles, and a `compile_fail` doctest shows it. What a type can't rule out
is listed under [Not caught at compile time](#not-caught-at-compile-time).

## The machines and their bugs

| Component | Machine | Bug | Fixed | Mistake rejected at compile time |
|---|---|---|---|---|
| aizome-shell | SHB-12 lock screen | An expired password or a lockout read "That password didn't work" | yes | yes |
| aizome-sys | SYB-12 sleep delay | A lock confirmed after resume released the next sleep's delay | yes | yes |
| Sukumo | UIW-27 session lock | A withdrawn lock's end ended the next lock | yes | yes |
| aizome-sys | SYB-09 polkit conversation | An unreachable helper was restarted as fast as the loop ran | yes | yes |
| aizome-shell | SHB-03 polkit dialog | The dialog restarted every failed attempt at once | yes | yes, apart from a person's actions (below) |
| aizome-greeter | SVC-05 greeter stage | A mistyped user name couldn't be corrected, and a failed session start was a dead end | yes | yes, apart from the view and the key bindings (below) |
| aizome-face, aizome-kit | SYC-07 secret text fields | An input method's composition was drawn in clear in password fields | yes | yes |
| aizome-kit and its users | A person's agreement | A destructive step could be confirmed by program code | yes | yes, apart from synthetic input (below) |

The reviews also found two bugs that main has today:

- **A polkit answer could reach another request's helper.** Tickets and
  prompt numbers were counted per request from 1, and `respond` didn't
  compare the request. With two requests open, one's password went to the
  other's helper, and polkitd authorized the other request. An answer now
  goes only through the request whose prompt it answers.
- **A Trash item's info could be forgotten for a later item's** (tier 1's
  HAKO-08 code, fixed in #290). ext4 gives a freed inode to the next file,
  so the inode alone didn't tell the two infos apart.

## How "caught at compile time" was shown

As in tier 1:

1. a `compile_fail` doctest per bug, writing its own mistake against the new
   API, beside a twin that compiles;
2. error codes checked with `RUSTC_BOOTSTRAP=1 cargo test --doc`;
3. the catalogued reproduction as a regression test, failing on the base and
   passing here;
4. an independent review per component that tried every other way to write
   each mistake, then re-verified the fixes.

The reviews found holes in every component's first version: the bug's own
line, pasted where it was first written (an app handler or a view), still
compiled. Those routes are now closed or listed below.

## Each machine

### Lock screen and lock-before-sleep (SHB-12, SYB-12, UIW-27)

- **SHB-12:** the PAM check returns PAM's own facts (the step, its code, what
  the modules said). The lock screen's note comes from `pam_done`, which takes
  only a checked answer, so the router can't pick "wrong password" for a
  lockout. The note is opaque, and the view draws it, not a string.
- **SYB-12:** each `PrepareForSleep` carries a ticket of its sleep cycle.
  Releasing logind's delay takes that ticket, which is spent once and refused
  after resume. The other release, `release_sleep_delay`, is gone.
- **UIW-27:** Sukumo names each session-lock request. The render thread
  answers with `LockConfirmed` or `LockFinished`, which only it makes, and
  each transition takes only its own kind, so neither a withdrawn lock's end
  nor a refusal taken as a confirmation can move the next lock.

### polkit agent and dialog (SYB-09, SHB-03)

- The agent starts every attempt itself, behind a floor shared by all its
  requests and kept across bus reconnects. After any attempt that ends without
  success, the next waits 0.5, 1, 2, then 4 s. Three attempts with no answer
  stop automatic retries until a person asks again. Only a success or a quiet
  minute resets it. Starting a helper takes a `Dial` that only the floor
  makes.
- Helper events reach the conversation only with the proof that
  `Generation::check_answer` makes, as the user asked. An answer goes only
  through the request and prompt it answers. It is refused before it's
  written if it holds a line break or NUL, or is too long.
- The dialog has no transition that starts an attempt on an event. It offers
  Try Again once the agent stops. An identity can only be one polkitd
  offered for that request, and PAM's reason is shown when there is one.

### Greeter (SVC-05)

Each login carries its own person, so a failed one can begin again for the
same person, and every login has a way back to where its person was chosen: the
list, or the name as typed. The greeter's fields are private, and the view
draws a login only through a block that takes the way back from the greeter's
own login. `state.toml` is synced before the session starts.

### Secret text fields (SYC-07)

`field(..).secure()` and `text_field(..).password()` return distinct types,
`SecretField` and `PasswordField`. Of an input method's composition, a secret
field keeps only how many characters it has, so its paint can draw dots and
nothing else. Text input is enabled with the password purpose, which input
methods that honour it answer by typing directly. A composition lives only
while the field has the focus. A change of value or mode drops it, and the
input method is told to start over. What a widget keeps is tied to the mode
its type names.

### A person's agreement

The kit makes an `Agreed<T>` only when a person presses a button made with
`on_agree(target, msg)`, with the pointer or keyboard, and only for a press
that began on that button. The agreement carries what the button showed,
can't be cloned, and is spent once. Program code can't make one, and tests
get one only through the kit's `testing` feature, which no shipped build
enables. The destructive steps take it:

- **aizome-sys and Tansu:** `Confirm::new` (and `Confirm::resize`, with the
  sizes a resize goes between) takes an agreement to the disk's identity, so
  an identity read from live state can't be confirmed. A new partition's
  flags are set as UDisks makes it. Tansu runs only the plan its Apply sheet
  listed.
- **Hako:** Replace takes a consent that names its conflict and its job, made
  with a key only the job's start makes, so a kept agreement can't replace
  another item.
- **Genkan:** the review page confirms only with the person's press of Erase
  Disk and Install, carrying the review's ticket and the typed phrase.
- **Kakehashi** (GPUI, not the kit): the helper starts only with a plan file
  made from the press of the dialog's OK, taken by value.

## Not caught at compile time

### Runtime facts no type can know

- **SYB-12:** whether a resume came between the sleep event and the release.
  The check is inside the only release.
- **polkit:** which request instance an answer belongs to. A type can't name
  an instance, so the only write compares it.
- **SYC-07:** whether an input method honours the password purpose, and when
  its in-flight events stop after a change of mode.
- **SVC-05:** greetd's answers, checked with `Generation::check_answer`.

### What a type can't require

No type can require that a key is bound, or that an event handler calls a
transition. The types only constrain what a route does once it's called.
Each of these compiles, and a test catches it:

- **Greeter:** a guard added to `Greeter::back` or `Greeter::submit`, or a view
  that skips the login block. A whole throwaway `Greeter` passed to the block
  also compiles.
- **Lock screen:** routing a finished lock to `unlock_session`, or resetting
  the machine with its constructor (documented as `App::run`'s only).

### A proof shows input, not a person

- An agreement proves the toolkit received a press, not who made it.
  Virtual input, `uinput` and accessibility tools press buttons as a person
  does. Accessibility presses don't make one yet; they should once the kit's
  AccessKit actions are wired.
- **Kakehashi:** GPUI's public dispatch methods, and its dock menu and jump
  list, could press OK from program code. A clippy `disallowed-methods` ban,
  run in its Nix check, rejects them; a plain `cargo build` doesn't run it.
- **polkit:** Try Again, the identity menu and Return in the field compile
  inside an event handler. The floor bounds them to one helper every 4 s.
  Adopting the agreement there is a follow-up.

### Left as follow-ups

- **Greeter:** with exactly one person listed, there is still no way to type
  another account's name (as before). A "Not listed?" link would add one.
- **polkit:** a refusal after PAM spoke but before anyone typed (a
  fingerprint-only stack) still counts as "no answer".
- **Hotaru:** its terminal's password prompts still show a composition. It
  would need to read the pty's `ECHO` and switch the view's mode.
- **Hako:** Delete for good and Empty Trash don't take an agreement yet.

## Changes a person will notice

- **Lock screen:** it says why PAM refused (an expired password, a lockout,
  what a module said). Enter does nothing until the lock is confirmed. A
  sleep that arrives while locking waits for the confirmation, up to logind's
  delay limit.
- **polkit dialog:** "Trying again…", then Try Again once attempts stop;
  PAM's reason after a refusal; an answer it can't send says so.
- **Greeter:** Escape and "Log in as someone else" on the typed name's
  password page; Return after a failed start tries again; what was typed
  while "Checking…" is kept.
- **Password fields:** a composition shows as dots, and input methods that
  honour the purpose don't compose there.
- **Tansu:** Return no longer applies a plan; it moves to Apply. Apply is red
  only when the plan destroys data.
- **Genkan:** Return in the phrase field moves to Erase Disk and Install.
- **Hako:** Merge from code on a non-folder keeps both.

## Protocols and packaging

- **greetd, PAM and polkit's helper:** the messages are unchanged. Answers
  reach the helper only for a pending prompt, once each, and helper starts are
  paced.
- **Sukumo:** its lock requests and answers carry tickets, and text input
  carries a purpose. These are messages between Sukumo's own threads, so the
  change reaches a program when it restarts.
- **aizome-sys depends on aizome-kit**, so that only the kit can make an
  agreement. Every program that links aizome-sys already links the kit, and
  no helper does.
- **Aizome's package** installs the executables its build phase made, so the
  check phase's test-feature builds can't reach `$out`.
- **Test-only features:** the kit's and Sukumo's `testing`, and Kakehashi's
  `test-util`, are enabled only by dev-dependencies.

## Decisions for you

- The lock screen refuses an expired password, now with the reason. Unlock
  instead (the password is right and the session is open), or offer a
  password change?
- With fprintd, a wrong password now shows the module's message, which may
  be a fingerprint prompt. Keep it, or filter informational messages (which
  could hide faillock's lockout notice)?
- polkit's timings: 0.5, 1, 2, then 4 s, stopping after three unanswered
  attempts.
- A polkit dialog that loses its screen waits for one rather than cancelling.
- aizome-sys depending on aizome-kit, rather than a separate disks crate.
- A secret field draws one dot per composed character, as GTK does, rather
  than nothing until it's committed.

## Requests for the shared crate

No component edited `aizome-typestate`. They asked for:

- `machine!` states generic over a marker (the greeter's logins, the secret
  field's modes);
- a step-bound proof `Now<'s, K, A>` and a generated `check`;
- a single-use spend of a `Fresh` and a process-unique `Generation` identity,
  so a re-made machine never reissues a ticket a stale answer names;
- doc comments on `machine!` branches;
- a route token whose tag only the component makes.

Each is implemented locally for now.

## Code size

Non-blank, non-comment lines in the source files the change touches, unit
tests inside those files included:

| Component | Before | After | Change |
|---|---:|---:|---:|
| aizome-sys | 2,214 | 3,420 | +1,206 |
| aizome-greeter | 1,281 | 2,104 | +823 |
| aizome-kit | 8,733 | 9,528 | +795 |
| aizome-shell | 3,798 | 4,497 | +699 |
| Sukumo | 5,942 | 6,178 | +236 |
| Hako | 4,546 | 4,776 | +230 |
| aizome-face | 432 | 572 | +140 |
| Kakehashi | 2,497 | 2,608 | +111 |
| Genkan UI | 3,744 | 3,817 | +73 |
| Tansu | 4,820 | 4,878 | +58 |
| Settei, network agent | 878 | 878 | 0 |
| **Total** | 38,885 | 43,256 | **+4,371 (+11%)** |

aizome-sys grew most: polkit's conversation is now its own module, with the
floor, the request-bound answers and their unit tests.

## Checks

The check kinds are kept apart, as `AGENTS.md` asks.

- **Component tests**, on the integrated branch:

  | Workspace | Tests passed | Doctests passed, codes checked |
  |---|---:|---:|
  | Aizome (every crate) | 2,670 of 2,672 | 176 |
  | Sukumo | 253 | 10 |
  | Genkan UI | 55 | 13 |
  | Kakehashi | 90 | 30 |

  The two Aizome failures, Settei's `keyboard_setup` and `search_results`
  goldens, fail on `typestate-tier1` too: their text changed in #176. In an
  earlier run Koe's `saved_settings_apply_to_the_next_dictation_without_restart`
  failed once under load; it passes alone and in its binary, and Koe is
  untouched.

  Rebased onto main at `9a4e6f09`, with #310's redrawn Settei goldens,
  Aizome passed 2,704 of 2,705. The failure, Utsushi's
  `recent_items_come_first_once`, is a race in that untouched app's tests:
  they name their directories by process and second, so two that start
  together share one. It passed five reruns. Its 177 doctests and Genkan
  UI's 55 tests passed. The Nix checks run the doctests that must not
  compile, their codes checked (`nix/checked-doctests.sh`, from #290): 112
  in `.#aizome`, 7 in Sukumo's check, 8 in `.#genkan` and 18 in
  `.#kakehashi`.

  Clippy is clean on every package the change touches, with Kakehashi's run
  denying GPUI's input dispatch. In the whole Aizome workspace, it also flags
  Hotaru's tests for a lint this toolchain added; Hotaru is untouched.
- **Regression tests:** each fails on `typestate-tier1` and passes here.
- **Nix builds:** `.#aizome` (whose install now takes the build phase's
  executables), `.#uta` (built against this branch's kit and Sukumo),
  `.#genkan`, `.#kakehashi` (its check runs the dispatch ban) and
  `checks.sukumo` (the Wayland lock tests under sway).
- **VM checks**, in disposable NixOS VMs, on the integrated branch. The two
  checks that needed fixing ran again after their fixes, which change only
  the checks:
  - `aizome-checks.greeter` and `greeter-x11`;
  - `aizome-checks.session`;
  - `aizome-checks.network-auth` (the network agent's password prompt);
  - `aizome-checks.encrypted-volumes`, after giving its fixture only the
    output it makes: it had failed to build since Yukimi became its own
    output (#131);
  - `aizome-checks.tansu-resize` and `tansu-size`;
  - `aizome-login` (logging in through the Aizome greeter);
  - `aizome-ibus` and `aizome-ibus-policy` (input methods);
  - `kakehashi-vm`;
  - `aizome-checks.polkit-recovery`: two installs through the polkit
    dialog, a shell reload, polkit restarted with the dialog open, the agent
    paused through a restart (Try Again), and exactly three helper starts.
    It had failed after its first install on main as well: it looked for
    Yukimi with `pgrep -x aizome-yukimi`, but Yukimi is wrapped and runs as
    `.aizome-yukimi-wrapped`. It now matches Yukimi's path.
- **Hardware:** no physical hardware was tested.
